//! Copies of the chats file in the `backups` folder: one a day, and one
//! before each update. Safe start can put a copy back.

use super::{BACKUP_PREFIX, Db};
use crate::error::{Error, Result};
use crate::paths::AppPaths;
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Daily copies kept. The oldest goes when an eighth is made.
const KEEP_DAILY: usize = 7;
pub(super) const DAILY_PREFIX: &str = "daily-";
/// The files a restore moved aside go here, never to the bin.
const REPLACED_PREFIX: &str = "replaced-";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Snapshot {
    /// The file name in the `backups` folder.
    pub name: String,
    /// "daily", or "update" for a copy made before an update.
    pub kind: &'static str,
    pub created_ms: i64,
    pub bytes: u64,
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Writes a clean copy of the open database to `file`.
pub(super) async fn copy_into(pool: &SqlitePool, file: &Path) -> Result<()> {
    sqlx::query("VACUUM INTO ?")
        .bind(file.to_string_lossy().into_owned())
        .execute(pool)
        .await?;
    Ok(())
}

/// The time part of a copy's name: `<prefix>...-<nanoseconds>.db`.
fn stamp(name: &str, prefix: &str) -> Option<u128> {
    name.strip_prefix(prefix)?
        .strip_suffix(".db")?
        .rsplit('-')
        .next()?
        .parse()
        .ok()
}

/// Keeps the newest `keep` copies whose names start with `prefix`.
pub(super) fn prune(dir: &Path, prefix: &str, keep: usize) -> Result<()> {
    let mut copies: Vec<(u128, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| Some((stamp(&e.file_name().into_string().ok()?, prefix)?, e.path())))
        .collect();
    copies.sort_unstable_by_key(|c| std::cmp::Reverse(c.0));
    for (_, old) in copies.iter().skip(keep) {
        if let Err(e) = std::fs::remove_file(old) {
            log::warn!("could not delete the old copy {}: {e}", old.display());
        }
    }
    Ok(())
}

impl Db {
    /// A clean, plain (not encrypted) copy of the open database, for a
    /// backup zip: it must open on a new PC, without this PC's key.
    pub async fn plain_copy_to(&self, file: &Path) -> Result<()> {
        let _ = std::fs::remove_file(file);
        let mut conn = self.pool.acquire().await?;
        sqlx::query("ATTACH DATABASE ?1 AS plain KEY ''")
            .bind(file.to_string_lossy().into_owned())
            .execute(&mut *conn)
            .await?;
        let exported = sqlx::query("SELECT sqlcipher_export('plain')")
            .execute(&mut *conn)
            .await;
        sqlx::query("DETACH DATABASE plain")
            .execute(&mut *conn)
            .await?;
        exported?;
        Ok(())
    }

    /// Today's copy, made once a day (a UTC day). `None` when it exists.
    pub async fn daily_snapshot(&self, db_path: &Path) -> Result<Option<PathBuf>> {
        let day = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() / 86_400);
        self.snapshot_on_day(db_path, day).await
    }

    pub(super) async fn snapshot_on_day(
        &self,
        db_path: &Path,
        day: u64,
    ) -> Result<Option<PathBuf>> {
        let dir = db_path.with_file_name("backups");
        std::fs::create_dir_all(&dir)?;
        let today = format!("{DAILY_PREFIX}{day}-");
        let made = std::fs::read_dir(&dir)?
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(&today));
        if made {
            return Ok(None);
        }
        let file = dir.join(format!("{today}{}.db", nanos()));
        copy_into(&self.pool, &file).await?;
        prune(&dir, DAILY_PREFIX, KEEP_DAILY)?;
        Ok(Some(file))
    }
}

/// The copies in `backups`, newest first.
pub fn list(backups: &Path) -> Vec<Snapshot> {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return Vec::new();
    };
    let mut copies: Vec<Snapshot> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (kind, at) = [(DAILY_PREFIX, "daily"), (BACKUP_PREFIX, "update")]
                .iter()
                .find_map(|(prefix, kind)| Some((*kind, stamp(&name, prefix)?)))?;
            let bytes = e.metadata().ok().filter(std::fs::Metadata::is_file)?.len();
            Some(Snapshot {
                name,
                kind,
                created_ms: i64::try_from(at / 1_000_000).unwrap_or(i64::MAX),
                bytes,
            })
        })
        .collect();
    copies.sort_unstable_by_key(|c| std::cmp::Reverse(c.created_ms));
    copies
}

/// Puts the copy `name` back as the chats file. Only while the database is
/// closed (safe start). The current files move to `backups/replaced-<time>`.
pub async fn restore(paths: &AppPaths, name: &str) -> Result<()> {
    // Only a name from the list: no other path can be read.
    let copy = list(&paths.backups())
        .into_iter()
        .find(|c| c.name == name)
        .ok_or_else(|| Error::Invalid("this copy is gone".into()))?;
    let from = paths.backups().join(&copy.name);
    match Db::integrity_check_file(&from, &paths.key()?).await {
        Ok(Some(problems)) if problems.is_empty() => {}
        _ => {
            return Err(Error::Damaged(
                "this copy is damaged too. Pick an older one".into(),
            ));
        }
    }
    let db = paths.database();
    let aside = paths
        .backups()
        .join(format!("{REPLACED_PREFIX}{}", nanos()));
    std::fs::create_dir_all(&aside)?;
    // The database with its journal files, or none of them: a journal next
    // to the wrong database would be read into it.
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let file = PathBuf::from(format!("{}{suffix}", db.display()));
        if !file.exists() {
            continue;
        }
        let to = aside.join(file.file_name().unwrap_or_default());
        if let Err(e) = std::fs::rename(&file, &to) {
            for (back, at) in moved.iter().rev() {
                if let Err(e) = std::fs::rename(at, back) {
                    log::error!("could not move {} back: {e}", at.display());
                }
            }
            return Err(e.into());
        }
        moved.push((file, to));
    }
    let temp = db.with_extension("db.restore.tmp");
    let placed = std::fs::copy(&from, &temp).and_then(|_| std::fs::rename(&temp, &db));
    if let Err(e) = placed {
        let _ = std::fs::remove_file(&temp);
        for (back, at) in moved.iter().rev() {
            if let Err(e) = std::fs::rename(at, back) {
                log::error!("could not move {} back: {e}", at.display());
            }
        }
        return Err(e.into());
    }
    log::info!(
        "chats file restored from {}; the old files are in {}",
        copy.name,
        aside.display()
    );
    Ok(())
}
