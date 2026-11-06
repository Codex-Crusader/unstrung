//! SQLite storage. One connection, so the core is the only writer.

mod characters;
mod chats;
mod images;
mod kept;
mod memories;
mod messages;
mod personas;
pub mod snapshots;

pub use characters::{Character, CharacterFields, WorldEntry};
pub use chats::{Chat, ChatSummary, DeletedChat, RecentChat};
pub use images::{GalleryImage, Image, NewImage};
pub use kept::{CharacterMemory, Kept, MEMORY_MODES};
pub use memories::Memory;
pub use messages::{Message, NewMessage};
pub use personas::{Persona, PersonaFields};

use crate::error::{Error, Result};
use crate::vault::DataKey;
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
    /// A copy of the chats before an app update installs. It goes with the
    /// copies from before a migration, so safe start can put it back.
    pub async fn update_snapshot(&self, db_path: &Path) -> Result<PathBuf> {
        backup(&self.pool, db_path, newest_schema()).await
    }

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

/// The `key` pragma value for SQLCipher.
fn key_pragma(key: &DataKey) -> String {
    format!("\"{}\"", key.database_key())
}

/// Options for a database file: with the key, or without it for a plain
/// file from before encryption (read-only checks of old copies).
fn file_options(path: &Path, key: &DataKey) -> SqliteConnectOptions {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .busy_timeout(Duration::from_secs(5));
    if crate::vault::is_plain_database(path) {
        options
    } else {
        // sqlx sends `key` before every other pragma, as SQLCipher needs.
        options.pragma("key", key_pragma(key))
    }
}

/// Encrypts a plain chats file in place: an encrypted copy next to it,
/// checked, then moved over it. The plain file stays until the copy is
/// good, so a stop at any point loses nothing.
pub(crate) async fn encrypt_plain(path: &Path, key: &DataKey) -> Result<()> {
    let new = PathBuf::from(format!("{}.encrypting", path.display()));
    let _ = std::fs::remove_file(&new);
    let plain = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                // ATTACH opens with these flags: it must create the copy.
                .create_if_missing(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let copied = async {
        if !quick_check(&plain).await? {
            return Err(Error::Damaged(
                "the chats file is damaged, so it was not encrypted. Safe start can put back a copy"
                    .into(),
            ));
        }
        // Everything into the main file, so no journal is needed after this.
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&plain)
            .await?;
        sqlx::query("ATTACH DATABASE ?1 AS encrypted KEY ?2")
            .bind(new.to_string_lossy().into_owned())
            .bind(key.database_key())
            .execute(&plain)
            .await?;
        sqlx::query("SELECT sqlcipher_export('encrypted')")
            .execute(&plain)
            .await?;
        sqlx::query("DETACH DATABASE encrypted")
            .execute(&plain)
            .await?;
        Ok(())
    }
    .await;
    plain.close().await;
    let checked = match copied {
        Ok(()) => Db::integrity_check_file(&new, key).await,
        Err(e) => Err(e),
    };
    match checked {
        Ok(Some(problems)) if problems.is_empty() && !crate::vault::is_plain_database(&new) => {}
        Ok(_) => {
            let _ = std::fs::remove_file(&new);
            return Err(Error::Damaged(
                "the encrypted copy of the chats file did not pass its check".into(),
            ));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
    }
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    std::fs::rename(&new, path)?;
    log::info!("encrypted {}", path.display());
    Ok(())
}

/// Encrypts the plain copies of the chats file in `backups` and its aside
/// folders, made before encryption. A damaged copy stays as it is. Returns
/// how many it encrypted.
pub async fn encrypt_old_copies(backups: &Path, key: &DataKey) -> usize {
    let entries = |d: &Path| {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect::<Vec<_>>()
    };
    let files: Vec<PathBuf> = entries(backups)
        .into_iter()
        .flat_map(|p| if p.is_dir() { entries(&p) } else { vec![p] })
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .filter(|p| crate::vault::is_plain_database(p))
        .collect();
    let mut done = 0;
    for file in files {
        match encrypt_plain(&file, key).await {
            Ok(()) => done += 1,
            Err(e) => log::warn!("could not encrypt {}: {e}", file.display()),
        }
    }
    done
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
    snapshots::copy_into(pool, &file).await?;
    snapshots::prune(&dir, BACKUP_PREFIX, KEEP_BACKUPS)?;
    Ok(file)
}

/// The highest migration that this build knows.
pub fn newest_schema() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or_default()
}

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// For tests that set a time in the past.
    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Opens (or creates) the database and runs the migrations.
    /// Opens the chats file with `key`. A plain file from before encryption
    /// (an old install, an old copy, a backup zip) is encrypted first.
    pub async fn open(path: &Path, key: &DataKey) -> Result<Self> {
        if crate::vault::is_plain_database(path) {
            encrypt_plain(path, key).await?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .pragma("key", key_pragma(key))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        // Plain SQLite ignores the key pragma without an error, so a build
        // that lost SQLCipher would write the chats in the clear. Refuse.
        #[cfg(windows)]
        if sqlx::query_scalar::<_, String>("PRAGMA cipher_version")
            .fetch_optional(&pool)
            .await?
            .is_none()
        {
            pool.close().await;
            return Err(Error::Invalid(
                "this build of Unstrung cannot encrypt the chats file".into(),
            ));
        }
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
                // With a copy to go back to, safe start offers it. Without
                // one, the chats stay usable as they are.
                log::error!("the database failed its quick check");
                if !snapshots::list(&path.with_file_name("backups")).is_empty() {
                    pool.close().await;
                    return Err(Error::Damaged(
                        "the chats file is damaged. Safe start can put back a copy".into(),
                    ));
                }
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
    pub async fn integrity_check_file(path: &Path, key: &DataKey) -> Result<Option<Vec<String>>> {
        if !path.exists() {
            return Ok(None);
        }
        let options = file_options(path, key).read_only(true);
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

    /// The highest migration of a database file that is not open, read
    /// without changes. An error when it is not an Unstrung database.
    pub async fn schema_of_file(path: &Path, key: &DataKey) -> Result<i64> {
        let options = file_options(path, key).read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let version: Result<Option<i64>> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .map_err(Error::from);
        pool.close().await;
        version?.ok_or_else(|| Error::Invalid("no migrations".into()))
    }

    /// The stored path of every picture, for the backup check.
    pub async fn image_paths(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar!("SELECT file_path FROM images")
            .fetch_all(&self.pool)
            .await?)
    }

    /// Characters, personas, chats (not deleted) and pictures.
    pub async fn counts(&self) -> Result<[i64; 4]> {
        let row = sqlx::query!(
            r#"SELECT (SELECT COUNT(*) FROM characters) AS "characters!: i64",
                      (SELECT COUNT(*) FROM personas) AS "personas!: i64",
                      (SELECT COUNT(*) FROM chats WHERE deleted_at IS NULL) AS "chats!: i64",
                      (SELECT COUNT(*) FROM images) AS "images!: i64""#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok([row.characters, row.personas, row.chats, row.images])
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
pub(crate) mod tests;
