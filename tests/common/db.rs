//! A fresh database for each test, cloned from a template. `#[sqlx::test]` ran all the
//! migrations into every test's database, which was most of what a test cost; a clone of an
//! already migrated database takes milliseconds.
//!
//! The template is named after the migrations it holds, so a changed or added migration
//! builds a new one. It is built once, under an advisory lock, by whichever process gets
//! there first. Each test's database is dropped when the test passes; one from a failed test
//! is left to look at, and swept by the next run once its process is gone.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection, PgPool};
use tokio::sync::{OnceCell, OwnedSemaphorePermit, Semaphore};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// The template's name, once it exists.
static TEMPLATE: OnceCell<String> = OnceCell::const_new();

/// Test databases alive at once in this process. Each holds a pool of up to five
/// connections, and Postgres allows a hundred by default.
static LIVE: OnceCell<std::sync::Arc<Semaphore>> = OnceCell::const_new();

static NEXT: AtomicU32 = AtomicU32::new(0);

/// Prefix of every database this module makes; nothing else is ever swept.
const PREFIX: &str = "wstest_";

fn master_options() -> PgConnectOptions {
    // From `.env` too, as `#[sqlx::test]` read it.
    let url = dotenvy::var("DATABASE_URL").expect("DATABASE_URL must be set for the tests");
    PgConnectOptions::from_str(&url).expect("DATABASE_URL parses")
}

async fn master() -> PgConnection {
    PgConnection::connect_with(&master_options())
        .await
        .expect("connect to DATABASE_URL")
}

/// The template's name: the migrations it holds, by version and checksum.
fn template_name() -> String {
    let mut hasher = DefaultHasher::new();
    for migration in MIGRATOR.iter() {
        migration.version.hash(&mut hasher);
        migration.checksum.hash(&mut hasher);
    }
    format!("{PREFIX}template_{:016x}", hasher.finish())
}

/// Build the template if it is not there yet, and sweep what earlier runs left behind.
async fn ensure_template() -> String {
    let name = template_name();
    let mut conn = master().await;
    // Any constant will do, as long as every test process agrees on it.
    conn.execute("select pg_advisory_lock(7700770077)")
        .await
        .expect("take the template lock");

    let exists: bool =
        sqlx::query_scalar("select exists(select 1 from pg_database where datname = $1)")
            .bind(&name)
            .fetch_one(&mut conn)
            .await
            .expect("look for the template");
    if !exists {
        // Built under another name and renamed when done, so a run that dies half way
        // leaves nothing that looks finished.
        let building = format!("{name}_building");
        drop_database(&mut conn, &building).await;
        conn.execute(AssertSqlSafe(format!("create database \"{building}\"")))
            .await
            .expect("create the template");
        let mut template = PgConnection::connect_with(&master_options().database(&building))
            .await
            .expect("connect to the template");
        MIGRATOR
            .run(&mut template)
            .await
            .expect("migrate the template");
        template.close().await.expect("close the template");
        conn.execute(AssertSqlSafe(format!(
            "alter database \"{building}\" rename to \"{name}\""
        )))
        .await
        .expect("finish the template");
    }
    sweep(&mut conn, &name).await;

    conn.execute("select pg_advisory_unlock(7700770077)")
        .await
        .expect("release the template lock");
    conn.close().await.ok();
    name
}

/// Drop older templates, and the databases of test processes that are no longer running.
/// Only where a process can be looked up by pid; elsewhere they stay.
async fn sweep(conn: &mut PgConnection, template: &str) {
    if !std::path::Path::new("/proc/self").exists() {
        return;
    }
    let names: Vec<String> =
        sqlx::query_scalar("select datname from pg_database where datname like 'wstest\\_%'")
            .fetch_all(&mut *conn)
            .await
            .expect("list test databases");
    for name in names {
        let stale = match name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.split_once('_'))
        {
            Some(("template", _)) => name != template,
            Some((pid, _)) => pid
                .parse::<u32>()
                .is_ok_and(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()),
            None => false,
        };
        if stale {
            drop_database(conn, &name).await;
        }
    }
}

async fn drop_database(conn: &mut PgConnection, name: &str) {
    conn.execute(AssertSqlSafe(format!(
        "drop database if exists \"{name}\" with (force)"
    )))
    .await
    .expect("drop a test database");
}

/// Holds a test's database for as long as the test runs, and drops it when the test passes.
/// Bind it to a name (`_db`, not `_`), or it goes at once.
pub(crate) struct TestDb {
    name: String,
    _live: OwnedSemaphorePermit,
}

/// A test's own database, migrated and empty, and the guard that keeps it.
pub(crate) async fn fresh() -> (PgPool, TestDb) {
    let template = TEMPLATE.get_or_init(ensure_template).await;
    let live = std::sync::Arc::clone(
        LIVE.get_or_init(|| async { std::sync::Arc::new(Semaphore::new(16)) })
            .await,
    )
    .acquire_owned()
    .await
    .expect("the semaphore is never closed");

    let name = format!(
        "{PREFIX}{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let mut conn = master().await;
    conn.execute(AssertSqlSafe(format!(
        "create database \"{name}\" template \"{template}\""
    )))
    .await
    .expect("clone the template");
    conn.close().await.ok();

    // As `#[sqlx::test]` had it: a few connections, let go of quickly.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .idle_timeout(Some(Duration::from_secs(1)))
        .connect_lazy_with(master_options().database(&name));
    (pool, TestDb { name, _live: live })
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // A failed test's database stays to be looked at; the next run sweeps it.
        if std::thread::panicking() {
            return;
        }
        let name = std::mem::take(&mut self.name);
        // Drop runs inside the test's runtime, which cannot be blocked on, so the drop
        // happens on a thread of its own.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime to drop the database on")
                .block_on(async {
                    let mut conn = master().await;
                    drop_database(&mut conn, &name).await;
                    conn.close().await.ok();
                });
        })
        .join()
        .ok();
    }
}
