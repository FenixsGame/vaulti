use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::time::Duration;

pub async fn new_pool(path: &str) -> sqlx::Result<SqlitePool> {
    let mut o = SqliteConnectOptions::new()
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    match path {
        ":memory:" => o = o.shared_cache(true),
        p => o = o.filename(p),
    }
    if path != ":memory:" {
        o = o
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
    }
    let max = if path == ":memory:" { 1 } else { 5 };
    SqlitePoolOptions::new()
        .max_connections(max)
        .connect_with(o)
        .await
}

pub async fn migrate(p: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("src/migrations").run(p).await
}
