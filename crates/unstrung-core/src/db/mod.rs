//! SQLite storage. One connection, so the core is the only writer.

mod characters;
mod chats;
mod memories;
mod messages;
mod personas;

pub use characters::{Character, CharacterFields, WorldEntry};
pub use chats::{Chat, ChatSummary};
pub use memories::Memory;
pub use messages::{Message, NewMessage};
pub use personas::{Persona, PersonaFields};

use crate::error::Result;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// "pending" while the welcome flow runs, then "done". "existing" on a
/// database from before the welcome flow.
pub const FIRST_RUN_KEY: &str = "first_run";
pub const FIRST_RUN_PENDING: &str = "pending";

impl Db {
    pub async fn first_run_pending(&self) -> Result<bool> {
        Ok(self.meta(FIRST_RUN_KEY).await?.as_deref() == Some(FIRST_RUN_PENDING))
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens (or creates) the database and runs the migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        let db = Self { pool };
        // A new database starts the welcome flow ("pending", until it ends
        // with "done"). A database from before the welcome flow existed has
        // an app_version from an earlier start, and is already set up.
        if db.meta(FIRST_RUN_KEY).await?.is_none() {
            let state = if db.meta("app_version").await?.is_some() {
                "existing"
            } else {
                FIRST_RUN_PENDING
            };
            db.set_meta(FIRST_RUN_KEY, state).await?;
        }
        let schema = MIGRATOR.iter().map(|m| m.version).max().unwrap_or_default();
        db.set_meta("schema_version", &schema.to_string()).await?;
        db.set_meta("app_version", env!("CARGO_PKG_VERSION"))
            .await?;
        db.set_meta("prompt_version", &crate::prompt::PROMPT_VERSION.to_string())
            .await?;
        Ok(db)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// A full `integrity_check` of a database file that is not open, on a
    /// read-only connection with no migrations, so a check never changes the
    /// file. `None` means there is no database yet. An empty list means it passed.
    pub async fn integrity_check_file(path: &Path) -> Result<Option<Vec<String>>> {
        if !path.exists() {
            return Ok(None);
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let rows: Vec<(String,)> = sqlx::query_as("PRAGMA integrity_check(20)")
            .fetch_all(&pool)
            .await?;
        pool.close().await;
        Ok(Some(
            rows.into_iter()
                .map(|(row,)| row)
                .filter(|row| row != "ok")
                .collect(),
        ))
    }

    pub async fn quick_check(&self) -> Result<bool> {
        let row: (String,) = sqlx::query_as("PRAGMA quick_check")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 == "ok")
    }

    pub async fn meta(&self, key: &str) -> Result<Option<String>> {
        let value = sqlx::query_scalar!("SELECT value FROM meta WHERE key = ?", key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(value)
    }

    pub async fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query!(
            "INSERT INTO meta (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
