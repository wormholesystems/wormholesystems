//! The live feed: zKillboard's R2Z2 stream, one killmail at a time.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sqlx::PgPool;
use tokio::sync::mpsc;

use super::{Detail, EsiKillmail, KillmailRow, extract_detail, extract_orgs, http_client};
use crate::background::Streak;
use crate::entities::EntityKind;
use crate::esi::EsiClient;

#[derive(Deserialize)]
struct R2Z2Sequence {
    sequence: i64,
}

#[derive(Deserialize)]
struct R2Z2Killmail {
    killmail_id: i64,
    hash: String,
    esi: EsiKillmail,
    /// zKillboard's own summary: value, attacker count, and the solo/NPC flags. Absent on
    /// a malformed frame, which is why every field below is read defensively.
    #[serde(default)]
    zkb: serde_json::Value,
}

/// Kills waiting for alert evaluation. Past this, evaluation is minutes behind (the
/// database is struggling), and an alert about a kill that old is not worth queueing.
const ALERT_QUEUE: usize = 1024;

/// Kills evaluated together, so a burst reads each map's chain once.
const ALERT_BATCH: usize = 100;

/// How far behind the head of the feed, in kills, before it is worth a warning. About 45
/// minutes of kills; a restart catches up on that in under two.
const BEHIND_WARN: i64 = 500;

/// Missing kills up to this many are stepped over one by one. Past it, the start of what
/// the feed still holds is searched for instead (see [`resume_at`]).
const WALK_LIMIT: i64 = 100;

/// How long the feed may keep failing before that is an error rather than a warning. A
/// failed fetch is retried after 30s, and zKillboard has bad minutes.
const FAILING_FOR: Duration = Duration::from_mins(15);

/// The longest wait after a 429, whatever `Retry-After` says. R2Z2 bans an address for an
/// hour, so past that the feed is only being asked again too late.
const THROTTLED_AT_MOST: Duration = Duration::from_hours(1);

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The feed answered 429. Every app on the address counts against the same limit, so this
/// can come from a neighbour's polling as much as ours.
#[derive(Debug)]
struct Throttled {
    retry_after: Option<Duration>,
}

impl Display for Throttled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.retry_after {
            Some(wait) => write!(
                f,
                "rate limited by the feed (retry after {}s)",
                wait.as_secs()
            ),
            None => f.write_str("rate limited by the feed"),
        }
    }
}

impl std::error::Error for Throttled {}

/// A response, or the error for its status: [`Throttled`] for a 429.
fn checked(res: reqwest::Response) -> Result<reqwest::Response, BoxError> {
    if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after);
        return Err(Throttled { retry_after }.into());
    }
    Ok(res.error_for_status()?)
}

/// `Retry-After` as seconds. The HTTP-date form is not worth a date parser here: without
/// it the wait is the backoff, which is long enough.
fn parse_retry_after(header: &str) -> Option<Duration> {
    let seconds: f64 = header.trim().parse().ok()?;
    // `NaN` and `inf` parse as floats too; `NaN.max(0.0)` would quietly read as zero.
    if !seconds.is_finite() {
        return None;
    }
    Duration::try_from_secs_f64(seconds.max(0.0)).ok()
}

/// How long to wait before asking the feed again.
///
/// Any failure but a 429 is retried after `retry`. Each 429 in a row doubles that, and the
/// wait is never shorter than `Retry-After` asks nor longer than [`THROTTLED_AT_MOST`]: one
/// that keeps coming is a block on the address, and asking every 30 seconds only keeps
/// it going.
#[derive(Debug, Default)]
struct Backoff {
    throttled: u32,
}

impl Backoff {
    fn after(&mut self, err: &BoxError, retry: Duration) -> Duration {
        let Some(throttled) = err.downcast_ref::<Throttled>() else {
            self.throttled = 0;
            return retry;
        };
        self.throttled = self.throttled.saturating_add(1);
        throttled_wait(retry, self.throttled, throttled.retry_after)
    }

    fn reset(&mut self) {
        self.throttled = 0;
    }
}

/// The wait after the `nth` 429 in a row (from 1).
fn throttled_wait(retry: Duration, nth: u32, retry_after: Option<Duration>) -> Duration {
    let doubled = retry.saturating_mul(2u32.saturating_pow(nth - 1));
    doubled
        .max(retry_after.unwrap_or_default())
        .min(THROTTLED_AT_MOST)
}

/// Log a failed step of the feed: a warning, or the one error once it has kept failing.
/// The error goes in a field so the reports group as one issue.
fn failed(streak: &Streak, err: impl Display, wait: Duration) {
    let retry_in = wait.as_secs();
    if streak.record_failure(Instant::now()) {
        tracing::error!(
            error = %err,
            retry_in,
            "killmail ingest has failed for {} minutes",
            FAILING_FOR.as_secs() / 60
        );
    } else {
        tracing::warn!(error = %err, retry_in, "killmail ingest failed, retrying");
    }
}

/// Log a failed step, and wait out the backoff it earned.
async fn retry_later(streak: &Streak, backoff: &mut Backoff, err: &BoxError, timing: Timing) {
    let wait = backoff.after(err, timing.retry);
    if err.is::<Throttled>() {
        metrics::counter!("ws_killmail_ingest_throttled_total").increment(1);
    }
    failed(streak, err, wait);
    tokio::time::sleep(wait).await;
}

/// How the feed is paced. Its own type so the tests can run it in milliseconds.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long to wait after the stream had nothing new.
    pub idle: Duration,
    /// The least time between two fetches while there is a backlog.
    pub pace: Duration,
    /// How often the cursor is compared with the head of the feed.
    pub head_check: Duration,
    /// How long a kill the head already counts may stay missing before it is given up on.
    pub missing_grace: Duration,
    /// How long to wait after a failed fetch. A 429 waits longer; see [`Backoff`].
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            // zKillboard asks for 6 seconds: a kill arrives every 5.5 on average, so
            // polling faster gains nothing.
            idle: Duration::from_secs(6),
            // R2Z2 allows 15 requests a second per address and bans it for an hour past
            // that. Other apps on the same address (a killfeed, say) catch up after an
            // outage at the same moment we do, so this leaves them most of the budget.
            pace: Duration::from_millis(250),
            head_check: Duration::from_secs(60),
            // The kill was asked for before it existed, while idling at the head, so a
            // cache in front of the bucket may answer that 404 again for a little while
            // after the file arrived. Five tries before it counts as gone.
            missing_grace: Duration::from_secs(30),
            retry: Duration::from_secs(30),
        }
    }
}

/// Follow the feed from where it was left off, for good.
///
/// A 404 is normally "nothing new yet", and waited out. But the feed does not keep its
/// files forever, and a kill can go missing from it: a 404 for a sequence the head has
/// already passed would then be waited on forever while the feed moves on. So the cursor
/// is compared with the head every so often, and a kill the head counts that stays missing
/// past [`Timing::missing_grace`] is skipped to what the feed still has.
#[doc(hidden)]
pub async fn listen(
    pool: PgPool,
    alerts: Option<Arc<crate::alerts::Runtime>>,
    base: String,
    timing: Timing,
) {
    let http = http_client();
    let alerts = alerts.map(|runtime| {
        let (tx, rx) = mpsc::channel(ALERT_QUEUE);
        // Ends with the sender, so a restarted feed does not leave the old one behind.
        tokio::spawn(alert_loop(pool.clone(), runtime, rx));
        tx
    });
    // Read from the table once, then kept here: the table is only for the next boot.
    let mut cursor: Option<i64> = None;
    let mut head = Head::default();
    // The kill that is missing under the head, and since when.
    let mut missing: Option<(i64, Instant)> = None;
    let streak = Streak::new(FAILING_FOR);
    let mut backoff = Backoff::default();
    loop {
        let last = match cursor {
            Some(seq) => seq,
            None => match start_at(&pool, &http, &base).await {
                Ok(seq) => {
                    cursor = Some(seq);
                    seq
                }
                Err(err) => {
                    retry_later(&streak, &mut backoff, &err, timing).await;
                    continue;
                }
            },
        };
        if head.due(timing.head_check)
            && let Err(err) = head.check(&http, &base, last).await
        {
            retry_later(&streak, &mut backoff, &err, timing).await;
            continue;
        }
        let next = last + 1;
        match ingest(&pool, &http, &base, alerts.as_ref(), next).await {
            Ok(true) => {
                streak.record_success();
                backoff.reset();
                cursor = Some(next);
                missing = None;
                tokio::time::sleep(timing.pace).await;
            }
            Ok(false) => {
                streak.record_success();
                backoff.reset();
                // Past the head: it has not happened yet.
                let Some(to) = head.seq.filter(|&head| next <= head) else {
                    tokio::time::sleep(timing.idle).await;
                    continue;
                };
                let since = match missing {
                    Some((seq, since)) if seq == next => since,
                    _ => missing.insert((next, Instant::now())).1,
                };
                if since.elapsed() < timing.missing_grace {
                    tokio::time::sleep(timing.idle).await;
                    continue;
                }
                match resume_at(&http, &base, next, to, timing.pace).await {
                    Ok(resume) => {
                        tracing::warn!(
                            "killmail ingest: {next}..={} are not in the feed, skipping to {resume}",
                            resume - 1
                        );
                        metrics::counter!("ws_killmail_ingest_skipped_total")
                            .increment(u64::try_from(resume - next).unwrap_or(0));
                        if let Err(err) = advance(&pool, resume - 1).await {
                            failed(&streak, err, Duration::ZERO);
                        }
                        cursor = Some(resume - 1);
                        missing = None;
                    }
                    Err(err) => {
                        metrics::counter!("ws_killmail_ingest_errors_total").increment(1);
                        retry_later(&streak, &mut backoff, &err, timing).await;
                    }
                }
            }
            Err(err) => {
                metrics::counter!("ws_killmail_ingest_errors_total").increment(1);
                retry_later(&streak, &mut backoff, &err, timing).await;
            }
        }
    }
}

/// The last sequence the feed has published, as of the last look.
#[derive(Default)]
struct Head {
    seq: Option<i64>,
    checked: Option<Instant>,
}

impl Head {
    fn due(&self, every: Duration) -> bool {
        self.checked.is_none_or(|at| at.elapsed() >= every)
    }

    /// Read the head again, and say how far behind it `cursor` is. A failed read keeps
    /// what was known and waits for the next check, except a 429, which is handed back so
    /// the feed is left alone for a while.
    async fn check(
        &mut self,
        http: &reqwest::Client,
        base: &str,
        cursor: i64,
    ) -> Result<(), BoxError> {
        self.checked = Some(Instant::now());
        match head_of(http, base).await {
            Ok(seq) => self.seq = Some(seq),
            Err(err) if err.is::<Throttled>() => return Err(err),
            Err(err) => {
                tracing::warn!("killmail ingest: could not read the head of the feed: {err}");
                return Ok(());
            }
        }
        let behind = self.seq.map_or(0, |head| (head - cursor).max(0));
        metrics::gauge!("ws_killmail_ingest_behind").set(to_f64(behind));
        if behind > BEHIND_WARN {
            tracing::warn!("killmail ingest is {behind} kills behind the feed (at {cursor})");
        }
        Ok(())
    }
}

/// The last sequence the feed has published.
async fn head_of(http: &reqwest::Client, base: &str) -> Result<i64, BoxError> {
    let head = http
        .get(format!("{base}/ephemeral/sequence.json"))
        .send()
        .await?;
    let head: R2Z2Sequence = checked(head)?.json().await?;
    Ok(head.sequence)
}

/// Whether the feed still has kill `seq`.
async fn present(http: &reqwest::Client, base: &str, seq: i64) -> Result<bool, BoxError> {
    let res = http
        .get(format!("{base}/ephemeral/{seq}.json"))
        .send()
        .await?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    checked(res)?;
    Ok(true)
}

/// The first kill after `missing` that the feed still has, up to `head`; one past `head`
/// when it has none of them.
///
/// A short gap is walked, so a kill pulled from the middle of the feed costs only itself.
/// A long one is a cursor older than what the feed keeps (the files expire), where what
/// is left is everything from some sequence to the head: that boundary is found by
/// halving, a dozen fetches instead of walking a day of kills.
async fn resume_at(
    http: &reqwest::Client,
    base: &str,
    missing: i64,
    head: i64,
    pace: Duration,
) -> Result<i64, BoxError> {
    if head - missing <= WALK_LIMIT {
        for seq in missing + 1..=head {
            if present(http, base, seq).await? {
                return Ok(seq);
            }
            tokio::time::sleep(pace).await;
        }
        return Ok(head + 1);
    }
    // `lo` is missing; `hi` is there, or one past the head.
    let (mut lo, mut hi) = (missing, head + 1);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if present(http, base, mid).await? {
            hi = mid;
        } else {
            lo = mid;
        }
        tokio::time::sleep(pace).await;
    }
    Ok(hi)
}

/// Where the stream left off, or the live head of it on the very first run.
async fn start_at(pool: &PgPool, http: &reqwest::Client, base: &str) -> Result<i64, BoxError> {
    let cursor: Option<i64> = sqlx::query_scalar!("select sequence_id from zkb_state")
        .fetch_optional(pool)
        .await?;
    if let Some(seq) = cursor {
        return Ok(seq);
    }
    let head = head_of(http, base).await?;
    sqlx::query!(
        "insert into zkb_state (id, sequence_id) values (true, $1)",
        head
    )
    .execute(pool)
    .await?;
    Ok(head)
}

/// Fetch and persist killmail `next` in the sequence. Returns whether there was one.
async fn ingest(
    pool: &PgPool,
    http: &reqwest::Client,
    base: &str,
    alerts: Option<&mpsc::Sender<Pending>>,
    next: i64,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let res = http
        .get(format!("{base}/ephemeral/{next}.json"))
        .send()
        .await?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false); // not there: not yet, or no longer
    }
    let body = checked(res)?.text().await?;
    let km = match serde_json::from_str::<R2Z2Killmail>(&body) {
        Ok(km) => km,
        Err(err) => {
            // Empty/garbage frame: advance past it, but say so. A change upstream would
            // otherwise skip every kill without a word.
            metrics::counter!("ws_killmail_ingest_unparsed_total").increment(1);
            tracing::warn!("killmail ingest: skipping {next}, the frame does not parse: {err}");
            advance(pool, next).await?;
            return Ok(true);
        }
    };

    let solar_system_id = km.esi.solar_system_id.unwrap_or(0);
    let time = km.esi.killmail_time.clone().unwrap_or_default();
    if solar_system_id != 0 && !time.is_empty() {
        let row = KillmailRow {
            id: km.killmail_id,
            hash: km.hash,
            solar_system_id,
            time,
            orgs: serde_json::to_value(extract_orgs(&km.esi))?,
            detail: extract_detail(&km.esi, &km.zkb),
        };
        super::insert_rows(pool, std::slice::from_ref(&row), super::Source::Live).await?;
        metrics::counter!("ws_killmails_ingested_total").increment(1);
        if let Some(lag) = lag_seconds(&row.time) {
            metrics::gauge!("ws_killmail_ingest_lag_seconds").set(lag);
        }
        announce(pool, solar_system_id).await;
        if let Some(alerts) = alerts {
            offer(alerts, row);
        }
    }
    advance(pool, next).await?;
    Ok(true)
}

/// A stored kill on its way to the alerts.
struct Pending {
    killmail_id: i64,
    solar_system_id: i64,
    detail: Detail,
}

/// Hand a kill to the alert worker without waiting for it. A full queue drops the kill's
/// alerts, not the kill: it is stored either way, and ingest must never wait on Discord or
/// on a slow evaluation.
fn offer(alerts: &mpsc::Sender<Pending>, row: KillmailRow) {
    let pending = Pending {
        killmail_id: row.id,
        solar_system_id: row.solar_system_id,
        detail: row.detail,
    };
    if let Err(mpsc::error::TrySendError::Full(pending)) = alerts.try_send(pending) {
        metrics::counter!("ws_killmail_alerts_dropped_total").increment(1);
        tracing::warn!(
            "killmail alerts: queue full, no alerts for killmail {}",
            pending.killmail_id
        );
    }
}

/// Evaluate queued kills against the alerts, in batches of whatever has piled up. A batch
/// that panics loses its alerts, not the worker: the queue would close with it, and every
/// kill after that would go without alerts and without a word.
async fn alert_loop(
    pool: PgPool,
    alerts: Arc<crate::alerts::Runtime>,
    mut queue: mpsc::Receiver<Pending>,
) {
    let mut pending = Vec::with_capacity(ALERT_BATCH);
    while queue.recv_many(&mut pending, ALERT_BATCH).await > 0 {
        let batch = std::mem::replace(&mut pending, Vec::with_capacity(ALERT_BATCH));
        let (pool, alerts) = (pool.clone(), Arc::clone(&alerts));
        crate::background::guarded("killmail alerts", move || async move {
            // Most of the time nobody watches for kills, and naming them is wasted work.
            if !alerts.watches_killmails(&pool).await {
                return;
            }
            let kills = kills_for(&pool, &batch).await;
            let out = alerts.killmails(&pool, &kills).await;
            alerts.enqueue(&pool, out).await;
        })
        .await;
    }
}

/// The batch as the alerts read it. Names come from what is already stored, since "Someone
/// lost a Loki" now beats a complete message a minute later. One query names the whole
/// batch.
async fn kills_for(pool: &PgPool, batch: &[Pending]) -> Vec<crate::alerts::killmail::Kill> {
    use crate::alerts::filters::Candidates;
    let (mut characters, mut types) = (Vec::new(), Vec::new());
    for p in batch {
        characters.extend([
            p.detail.victim_character_id,
            p.detail.final_blow_character_id,
        ]);
        types.extend([
            p.detail.victim_ship_type_id,
            p.detail.final_blow_ship_type_id,
        ]);
    }
    let characters: Vec<i64> = characters.into_iter().flatten().collect();
    let types: Vec<i64> = types.into_iter().flatten().collect();
    let rows = sqlx::query!(
        r#"select false as "is_type!", id as "id!", name as "name!", null::bigint as group_id
           from characters where id = any($1)
           union all
           select true, id, name, group_id from types where id = any($2)"#,
        &characters,
        &types,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|err| {
        tracing::warn!("killmail alerts: could not name the kills: {err}");
        Vec::new()
    });
    let mut names = HashMap::new();
    let mut ships = HashMap::new();
    for row in rows {
        if row.is_type {
            ships.insert(row.id, (row.name, row.group_id));
        } else {
            names.insert(row.id, row.name);
        }
    }
    let name = |id: Option<i64>| id.and_then(|id| names.get(&id).cloned());
    let ship = |id: Option<i64>| id.and_then(|id| ships.get(&id));

    batch
        .iter()
        .map(|p| {
            let detail = &p.detail;
            let victim_ship = ship(detail.victim_ship_type_id);
            let attacker_ship = ship(detail.final_blow_ship_type_id);
            crate::alerts::killmail::Kill {
                id: p.killmail_id,
                solar_system_id: p.solar_system_id,
                candidates: Candidates {
                    victim_character: detail.victim_character_id,
                    victim_corporation: detail.victim_corporation_id,
                    victim_alliance: detail.victim_alliance_id,
                    victim_ship_type: detail.victim_ship_type_id,
                    victim_ship_group: victim_ship.and_then(|s| s.1),
                    attacker_character: detail.final_blow_character_id,
                    attacker_corporation: detail.final_blow_corporation_id,
                    attacker_alliance: detail.final_blow_alliance_id,
                    attacker_ship_type: detail.final_blow_ship_type_id,
                    attacker_ship_group: attacker_ship.and_then(|s| s.1),
                },
                victim_name: name(detail.victim_character_id),
                victim_ship: victim_ship.map(|s| s.0.clone()),
                victim_ship_type_id: detail.victim_ship_type_id,
                attacker_name: name(detail.final_blow_character_id),
                total_value: detail.total_value,
                attacker_count: detail.attacker_count,
                is_solo: detail.is_solo,
                is_npc: detail.is_npc,
            }
        })
        .collect()
}

/// Tell every map holding this system that something died in it. The event carries no
/// payload: what a client shows depends on its own filter, so it refetches.
async fn announce(pool: &PgPool, solar_system_id: i64) {
    let Ok(map_ids) = sqlx::query_scalar!(
        "select distinct map_id from map_solar_systems where solar_system_id = $1",
        solar_system_id,
    )
    .fetch_all(pool)
    .await
    else {
        return;
    };
    for map_id in map_ids {
        crate::realtime::hub().publish(crate::realtime::MapEvent::KillmailReceived { map_id });
    }
}

/// How long ago the kill happened, from its ESI timestamp: how far behind the feed is.
fn lag_seconds(killmail_time: &str) -> Option<f64> {
    let at = chrono::DateTime::parse_from_rfc3339(killmail_time).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(now.as_secs_f64() - to_f64(at.timestamp()))
}

/// For the gauges, which take `f64`.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "sequence numbers and Unix seconds stay far below 2^52, where f64 is exact"
)]
fn to_f64(n: i64) -> f64 {
    n as f64
}

async fn advance(pool: &PgPool, seq: i64) -> Result<(), sqlx::Error> {
    metrics::gauge!("ws_killmail_ingest_sequence").set(to_f64(seq));
    sqlx::query!("update zkb_state set sequence_id = $1", seq)
        .execute(pool)
        .await?;
    Ok(())
}

/// Killmails looked at per query. A busy minute is around a thousand.
const NAME_BATCH: i64 = 2_000;

/// Passes a batch is held back for while ESI fails on some of it, before the resolver gives
/// up on those names and moves on. Each pass is a minute apart.
const NAME_RETRIES: u32 = 3;

/// Put names to the ids on new killmails. A separate loop from the ingest: a killmail
/// must be recorded whether or not ESI is answering, and the names are only wanted by the
/// time someone opens the card.
///
/// Every kill in EVE is named, not only those in mapped systems. The card is the obvious
/// reader and only shows mapped systems, but a hole is mapped minutes after it is found and
/// its last week of kills should read as names right then. The same names also serve the
/// killmail alerts (which reach beyond the chain), the access search, and the threat
/// analysis. With the bulk endpoint that is a call or two a minute, so not worth narrowing.
pub(super) fn start_resolver(pool: PgPool, esi: EsiClient) {
    let resolver = Arc::new(tokio::sync::Mutex::new(NameResolver::default()));
    crate::background::spawn_every("killmail names", Duration::from_secs(60), move || {
        let (pool, esi, resolver) = (pool.clone(), esi.clone(), Arc::clone(&resolver));
        async move {
            resolver.lock().await.pass(&pool, &esi).await;
            Ok::<_, std::convert::Infallible>(())
        }
    });
}

/// Where the name resolver has got to: the last `ingest_seq` it is done with, read from
/// `zkb_state` once and kept here after, and how many passes the current batch has been
/// held back for.
#[derive(Default, Debug)]
pub struct NameResolver {
    mark: Option<i64>,
    held: u32,
}

impl NameResolver {
    /// Name everything stored since the last pass.
    pub async fn pass(&mut self, pool: &PgPool, esi: &EsiClient) {
        if self.mark.is_none() {
            self.mark = sqlx::query_scalar!("select named_seq from zkb_state")
                .fetch_optional(pool)
                .await
                .map(|seq| seq.unwrap_or(0))
                .inspect_err(|err| tracing::warn!("killmail names: {err}"))
                .ok();
        }
        if let Some(from) = self.mark {
            self.mark = Some(resolve_new(pool, esi, from, &mut self.held).await);
        }
    }
}

/// Name everything stored since `from`, and return how far that got. The mark only moves
/// past a batch once it is named, or once ESI has failed on it [`NAME_RETRIES`] times.
async fn resolve_new(pool: &PgPool, esi: &EsiClient, from: i64, held: &mut u32) -> i64 {
    let mut mark = from;
    loop {
        // Windowed to what the card can show: a kill zKillboard delivers late enough to be
        // past it is never on a card.
        let rows = match sqlx::query!(
            r#"select ingest_seq as "ingest_seq!",
                      victim_character_id, victim_corporation_id, victim_alliance_id,
                      final_blow_character_id, final_blow_corporation_id,
                      final_blow_alliance_id
               from killmails
               where ingest_seq > $1 and time >= now() - make_interval(days => $2)
               order by ingest_seq
               limit $3"#,
            mark,
            super::CARD_WINDOW_DAYS,
            NAME_BATCH,
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!("killmail names: {err}");
                return mark;
            }
        };
        let Some(last) = rows.last().map(|row| row.ingest_seq) else {
            return mark;
        };
        let full = i64::try_from(rows.len()) == Ok(NAME_BATCH);

        let mut characters = Vec::new();
        let mut corporations = Vec::new();
        let mut alliances = Vec::new();
        for row in rows {
            characters.extend(row.victim_character_id);
            characters.extend(row.final_blow_character_id);
            corporations.extend(row.victim_corporation_id);
            corporations.extend(row.final_blow_corporation_id);
            alliances.extend(row.victim_alliance_id);
            alliances.extend(row.final_blow_alliance_id);
        }
        // Organisations first: most rows named per fetch if a rate limit cuts the run
        // short. They go one at a time, because the ticker the card shows only comes from
        // the per-entity endpoint; characters need only a name.
        let failed = crate::entities::ensure(pool, esi, EntityKind::Alliance, &alliances)
            .await
            .failed
            + crate::entities::ensure(pool, esi, EntityKind::Corporation, &corporations)
                .await
                .failed
            + crate::entities::ensure_character_names(pool, esi, &characters)
                .await
                .failed;
        if failed > 0 && *held < NAME_RETRIES {
            *held += 1;
            return mark;
        }
        if failed > 0 {
            tracing::warn!(
                "killmail names: giving up on {failed} names after {NAME_RETRIES} tries"
            );
        }
        *held = 0;
        mark = last;
        if let Err(err) = sqlx::query!("update zkb_state set named_seq = $1", mark)
            .execute(pool)
            .await
        {
            tracing::warn!("killmail names: {err}");
        }
        if !full {
            return mark;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RETRY: Duration = Duration::from_secs(30);

    #[test]
    fn each_429_in_a_row_doubles_the_wait_up_to_an_hour() {
        let waits: Vec<u64> = (1..=9)
            .map(|nth| throttled_wait(RETRY, nth, None).as_secs())
            .collect();
        assert_eq!(waits, [30, 60, 120, 240, 480, 960, 1920, 3600, 3600]);
        assert_eq!(throttled_wait(RETRY, u32::MAX, None), THROTTLED_AT_MOST);
    }

    #[test]
    fn retry_after_lengthens_the_wait_but_never_past_an_hour() {
        let asked = |secs| Some(Duration::from_secs(secs));
        assert_eq!(
            throttled_wait(RETRY, 1, asked(300)),
            Duration::from_secs(300)
        );
        // A shorter Retry-After than the backoff does not cut it short.
        assert_eq!(throttled_wait(RETRY, 3, asked(5)), Duration::from_secs(120));
        assert_eq!(throttled_wait(RETRY, 1, asked(86_400)), THROTTLED_AT_MOST);
    }

    #[test]
    fn retry_after_is_a_finite_number_of_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(
            parse_retry_after(" 1.5 "),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(parse_retry_after("-3"), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("NaN"), None);
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }

    #[test]
    fn a_success_or_another_failure_starts_the_backoff_over() {
        let throttled: BoxError = Throttled { retry_after: None }.into();
        let other: BoxError = "connection reset".into();
        let mut backoff = Backoff::default();
        assert_eq!(backoff.after(&throttled, RETRY), RETRY);
        assert_eq!(backoff.after(&throttled, RETRY), RETRY * 2);
        assert_eq!(backoff.after(&other, RETRY), RETRY);
        assert_eq!(backoff.after(&throttled, RETRY), RETRY);
        backoff.after(&throttled, RETRY);
        backoff.reset();
        assert_eq!(backoff.after(&throttled, RETRY), RETRY);
    }
}
