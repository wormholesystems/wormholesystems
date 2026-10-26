//! Structure names: what a pilot docked in a player-owned structure is docked in.
//!
//! ESI lists every public structure's id without a token, but naming one needs a character
//! with the optional `esi-universe.read_structures.v1`. So once an hour this borrows a
//! random such character's token and names the public structures we have not named, or
//! named too long ago, a bounded batch at a time. Private ones are named by the poller with
//! the token of the pilot docked inside ([`crate::pilots::structures`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use sqlx::PgPool;

use crate::db::PgTokenStore;
use crate::esi::entities::Structure;
use crate::esi::{EsiClient, EsiError, Scope, Sso, TokenCache};
use crate::pilots::poller::run_bounded;
use crate::server_status::ServerWatch;

/// Structures change hands and names slowly; an hour keeps new ones turning up promptly.
const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Lookups per pass. The first passes work through the whole public list, so this is what
/// keeps that from being one burst against ESI.
const BATCH: i64 = 1000;

/// Max concurrent lookups. Never urgent.
const CONCURRENCY: usize = 8;

/// Spawn the sync loop. Returns immediately; the loop runs for the process lifetime.
pub fn start(
    pool: PgPool,
    sso: Arc<Sso>,
    tokens: Arc<TokenCache>,
    esi: EsiClient,
    server: ServerWatch,
) {
    crate::background::spawn_every("structure sync", INTERVAL, move || {
        let (pool, sso, tokens) = (pool.clone(), sso.clone(), tokens.clone());
        let (esi, server) = (esi.clone(), server.clone());
        async move {
            if server.should_poll() {
                sync_once(&pool, &sso, &tokens, &esi).await;
            }
            Ok::<_, std::convert::Infallible>(())
        }
    });
}

async fn sync_once(pool: &PgPool, sso: &Sso, tokens: &TokenCache, esi: &EsiClient) {
    let ids = match esi.public_structures().await {
        Ok(ids) => ids,
        Err(err) => {
            tracing::warn!("structure sync: could not list public structures: {err}");
            return;
        }
    };
    let wanted = match needing_names(pool, &ids, BATCH).await {
        Ok(wanted) if wanted.is_empty() => return,
        Ok(wanted) => wanted,
        Err(err) => {
            tracing::error!("structure sync: {err}");
            return;
        }
    };
    let Some(token) = borrow_token(pool, sso, tokens).await else {
        // Nobody has granted the permission yet. Nothing to do until someone does.
        return;
    };

    let named = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    // Once ESI says to back off, the rest of the batch waits for the next pass.
    let halted = Arc::new(AtomicBool::new(false));
    run_bounded(&wanted, CONCURRENCY, |id| {
        let (pool, esi, token) = (pool.clone(), esi.clone(), token.clone());
        let (named, failed, halted) = (named.clone(), failed.clone(), halted.clone());
        async move {
            if halted.load(Ordering::Relaxed) {
                return;
            }
            let outcome = match esi.structure(&token, id).await {
                Ok(structure) => store(&pool, id, &structure).await.map(|()| true),
                Err(EsiError::Api {
                    status: 403 | 404, ..
                }) => note_refused(&pool, id).await.map(|()| false),
                Err(err) => {
                    if matches!(
                        err,
                        EsiError::Backoff(_) | EsiError::Api { status: 401, .. }
                    ) {
                        halted.store(true, Ordering::Relaxed);
                    }
                    failed.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            match outcome {
                Ok(true) => {
                    named.fetch_add(1, Ordering::Relaxed);
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!("structure sync: could not store structure {id}: {err}");
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    })
    .await;

    let (named, failed) = (
        named.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed),
    );
    if failed > 0 {
        tracing::warn!("structure sync: named {named}, {failed} failed, retried next pass");
    } else {
        tracing::info!("structure sync: named {named} of {}", wanted.len());
    }
}

/// An access token from a random character who granted the permission. One whose stored
/// token is still valid is preferred, so borrowing it rarely costs a refresh. It goes
/// through the same [`TokenCache`] as the poller, so a refresh never runs beside theirs.
async fn borrow_token(pool: &PgPool, sso: &Sso, tokens: &TokenCache) -> Option<String> {
    let candidates = sqlx::query_scalar!(
        "select t.character_id
         from esi_tokens t
         join esi_token_scopes ts on ts.token_id = t.id
         join esi_scopes s on s.id = ts.scope_id
         where s.name = $1 and t.rejected_at is null
         order by t.token_expires_at > now() + interval '5 minutes' desc, random()
         limit 3",
        Scope::ReadStructures.as_str(),
    )
    .fetch_all(pool)
    .await
    .inspect_err(|err| tracing::error!("structure sync: could not pick a token: {err}"))
    .ok()?;
    let store = PgTokenStore::new(pool.clone());
    // A token EVE refused is marked on the way and the next candidate tried.
    for character_id in candidates {
        match tokens
            .token(sso, &store, character_id, &[Scope::ReadStructures])
            .await
        {
            Ok(token) if token.has_scope(Scope::ReadStructures) => return Some(token.access_token),
            Ok(_) => {
                tracing::debug!("structure sync: token for {character_id} lacks the scope")
            }
            Err(err) => {
                tracing::debug!("structure sync: token for {character_id} unusable: {err}")
            }
        }
    }
    None
}

/// Of `ids`, the ones worth asking about, at most `limit`: never named first, then those
/// named over a week ago. One ESI refused is left alone for a day.
pub async fn needing_names(pool: &PgPool, ids: &[i64], limit: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        r#"select w.id as "id!"
           from unnest($1::bigint[]) as w(id)
           left join structures s on s.id = w.id
           where s.id is null
              or s.updated_at < now() - case when s.name is null
                                             then interval '1 day'
                                             else interval '7 days' end
           order by s.id is not null, s.updated_at
           limit $2"#,
        ids,
        limit,
    )
    .fetch_all(pool)
    .await
}

/// Whether the structure has a name recent enough to keep. Older than a week is refetched,
/// since owners rename their structures.
pub async fn is_named(pool: &PgPool, structure_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"select exists(
               select 1 from structures
               where id = $1 and name is not null
                 and updated_at > now() - interval '7 days'
           ) as "named!""#,
        structure_id,
    )
    .fetch_one(pool)
    .await
}

/// Record what ESI said about the structure. A type or owner we do not have is left out
/// rather than failing the whole row: only the name is read.
pub async fn store(pool: &PgPool, structure_id: i64, structure: &Structure) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into structures (id, solar_system_id, name, type_id, owner_corporation_id)
         values ($1, $2, $3, (select id from types where id = $4),
                 (select id from corporations where id = $5))
         on conflict (id) do update
         set solar_system_id = excluded.solar_system_id, name = excluded.name,
             type_id = excluded.type_id, owner_corporation_id = excluded.owner_corporation_id,
             updated_at = now()",
        structure_id,
        structure.solar_system_id,
        structure.name,
        structure.type_id,
        structure.owner_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// ESI would not name it for the borrowed token. Stamped so the sync leaves it for a day; a
/// name it already had is kept, and a pilot docked inside can still name it.
pub async fn note_refused(pool: &PgPool, structure_id: i64) -> sqlx::Result<()> {
    sqlx::query!(
        "insert into structures (id) values ($1)
         on conflict (id) do update set updated_at = now()",
        structure_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
