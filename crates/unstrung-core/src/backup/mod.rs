//! "Export everything" and "Import backup". A backup is a zip with a
//! manifest, a clean copy of the chats file, the avatars and the gallery
//! pictures. Engines, models, logs and files about this PC (such as
//! `location.json`) stay out. Every setting is a row in the chats file.

use crate::backupkey::{BackupKey, Encryption};
use crate::db::{Character, CharacterFields, Db, Persona, PersonaFields, now_ms};
use crate::error::{Error, Result};
use crate::paths::{AppPaths, join_safe};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, ZipArchive};

mod add;
mod export;
mod open;
mod replace;
#[cfg(test)]
mod tests;

pub use add::*;
pub use export::*;
pub use open::*;
pub use replace::*;

pub const MANIFEST: &str = "unstrung-backup.json";
pub const DATABASE: &str = "app.db";
const FORMAT: u32 = 1;
/// An encrypted backup. An older app that knows only 1 says "Update Unstrung
/// first" instead of failing on the encrypted files.
const FORMAT_SEALED: u32 = 2;
/// The backup password, in the encrypted chats file. Every export uses it.
const PASSWORD: &str = "backup_password";
/// The only files besides the chats file and the manifest.
const PICTURE_TYPES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// Work files in `backups` that a crash can leave behind.
const LEFTOVER_PREFIXES: [&str; 2] = ["export-", IMPORT_PREFIX];
/// The staged chats file and avatars of an import, in `backups`.
const IMPORT_PREFIX: &str = "import-";
/// The staged pictures of an import, in the library folder, so that they
/// move into place on the same disk.
const GALLERY_IMPORT_PREFIX: &str = "gallery-import-";

// A backup comes from outside: limits for a zip bomb or a damaged file.
const MAX_ENTRIES: usize = 200_000;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_DATABASE_BYTES: u64 = 8 << 30;
const MAX_PICTURE_BYTES: u64 = 64 << 20;
const MAX_TOTAL_BYTES: u64 = 512 << 30;
/// Unpacked bytes per packed byte, for files over 1 MB. Real backups stay
/// far below it: pictures are stored as they are.
const MAX_RATIO: u64 = 1000;
/// Room that stays free on the disk after an export.
const SPARE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format: u32,
    /// The highest migration of the chats file.
    pub schema: i64,
    pub app_version: String,
    pub created_ms: i64,
    /// Only in an encrypted backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<Encryption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    pub bytes: u64,
    pub avatars: usize,
    pub pictures: usize,
}

/// When the last "Export everything" finished.
const LAST_BACKUP: &str = "last_backup_ms";
/// When the reminder clock started: the first check, or the last "Later".
const REMINDER_CLOCK: &str = "backup_reminder_ms";
/// How long the app waits before it reminds the user to make a backup.
pub const REMIND_AFTER_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    pub last_ms: Option<i64>,
    /// True when there are chats and no backup or "Later" in 30 days.
    pub due: bool,
    /// True when exports are encrypted with the backup password.
    pub password: bool,
}

async fn meta_ms(db: &Db, key: &str) -> Result<Option<i64>> {
    Ok(db.meta(key).await?.and_then(|v| v.parse().ok()))
}

pub async fn status(db: &Db, now: i64) -> Result<BackupStatus> {
    let last = meta_ms(db, LAST_BACKUP).await?;
    let clock = match meta_ms(db, REMINDER_CLOCK).await? {
        Some(t) => t,
        // The clock starts now, so an update does not remind at once.
        None => {
            db.set_meta(REMINDER_CLOCK, &now.to_string()).await?;
            now
        }
    };
    let since = last.map_or(clock, |l| l.max(clock));
    let chats = db.counts().await?[2];
    Ok(BackupStatus {
        last_ms: last,
        due: chats > 0 && now - since >= REMIND_AFTER_MS,
        password: password(db).await?.is_some(),
    })
}

async fn password(db: &Db) -> Result<Option<String>> {
    Ok(db.meta(PASSWORD).await?.filter(|p| !p.is_empty()))
}

/// Sets the backup password, or removes it with `None`. Backups made before
/// keep the password they were made with.
pub async fn set_password(db: &Db, new: Option<&str>) -> Result<()> {
    if let Some(p) = new
        && p.chars().count() < crate::backupkey::MIN_PASSWORD_CHARS
    {
        return Err(Error::Invalid(format!(
            "the password needs at least {} characters",
            crate::backupkey::MIN_PASSWORD_CHARS
        )));
    }
    db.set_meta(PASSWORD, new.unwrap_or_default()).await
}

/// "Later" on the reminder: ask again in 30 days.
pub async fn snooze(db: &Db, now: i64) -> Result<()> {
    db.set_meta(REMINDER_CLOCK, &now.to_string()).await
}

/// Removes the work files of an export or import that a crash left behind.
pub fn clear_leftovers(paths: &AppPaths) {
    let Ok(entries) = std::fs::read_dir(paths.backups()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !LEFTOVER_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let removed = match entry.file_type() {
            Ok(t) if t.is_file() => std::fs::remove_file(entry.path()),
            Ok(t) if t.is_dir() => std::fs::remove_dir_all(entry.path()),
            _ => Ok(()),
        };
        if let Err(e) = removed {
            log::warn!("could not delete the old work file {name}: {e}");
        }
    }
    // Staged pictures, in the library folder.
    let staged: Vec<String> = std::fs::read_dir(paths.library())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_prefix(GALLERY_IMPORT_PREFIX).map(str::to_owned)
        })
        .collect();
    for id in staged {
        discard(paths, &id);
    }
}
