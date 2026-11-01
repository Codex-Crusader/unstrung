//! SQLite storage. One connection, so the core is the only writer.

mod characters;
mod chats;
mod memories;
mod messages;
mod personas;

pub use characters::{Character, CharacterFields, WorldEntry};
pub use chats::{Chat, ChatSummary, RecentChat};
pub use memories::Memory;
pub use messages::{Message, NewMessage};
pub use personas::{Persona, PersonaFields};

use crate::error::{Error, Result};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Copies made before a migration. The newest ones stay.
const KEEP_BACKUPS: usize = 3;
const BACKUP_PREFIX: &str = "before-schema-";

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

async fn quick_check(pool: &SqlitePool) -> Result<bool> {
    let row: (String,) = sqlx::query_as("PRAGMA quick_check").fetch_one(pool).await?;
    Ok(row.0 == "ok")
}

/// Copies the database into the `backups` folder next to it (see
/// `AppPaths::backups`) and keeps the newest `KEEP_BACKUPS` copies.
async fn backup(pool: &SqlitePool, db_path: &Path, schema: i64) -> Result<PathBuf> {
    let dir = db_path.with_file_name("backups");
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let file = dir.join(format!("{BACKUP_PREFIX}{schema}-{stamp}.db"));
    sqlx::query("VACUUM INTO ?")
        .bind(file.to_string_lossy().into_owned())
        .execute(pool)
        .await?;
    let mut copies: Vec<(u128, PathBuf)> = std::fs::read_dir(&dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let rest = name.strip_prefix(BACKUP_PREFIX)?.strip_suffix(".db")?;
            Some((rest.rsplit('-').next()?.parse().ok()?, e.path()))
        })
        .collect();
    copies.sort_unstable_by_key(|c| std::cmp::Reverse(c.0));
    for (_, old) in copies.iter().skip(KEEP_BACKUPS) {
        if let Err(e) = std::fs::remove_file(old) {
            log::warn!("could not delete the old copy {}: {e}", old.display());
        }
    }
    Ok(file)
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
        let newest = MIGRATOR.iter().map(|m| m.version).max().unwrap_or_default();
        // A new database has no migrations table, so nothing is pending.
        let applied: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .ok()
            .flatten();
        let ok = quick_check(&pool).await?;
        match applied {
            Some(applied) if applied < newest => {
                // A migration writes to the file: never to a damaged one, and
                // never without a copy to go back to.
                if !ok {
                    return Err(Error::Damaged(
                        "the chats file is damaged, so this update does not change it".into(),
                    ));
                }
                let copy = backup(&pool, path, applied).await?;
                log::info!("database copied to {} before the update", copy.display());
            }
            _ if !ok => {
                // Phase 7 adds snapshots and the restore screen. Until then
                // the chats stay usable.
                log::error!("the database failed its quick check");
            }
            _ => {}
        }
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
        db.set_meta("schema_version", &newest.to_string()).await?;
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
        quick_check(&self.pool).await
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
