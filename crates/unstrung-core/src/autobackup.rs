//! Automatic backups: once a week, "Export everything" into a folder that
//! the user picked, and the 3 newest stay. The app's hourly task calls
//! `run_if_due`. Only files with this module's exact name pattern are ever
//! deleted, so the folder can hold other files.

use crate::backup::{self, Exported};
use crate::db::Db;
use crate::error::{Error, Result};
use crate::paths::AppPaths;
use serde::Serialize;
use std::path::{Path, PathBuf};

const FOLDER: &str = "auto_backup_dir";
const DONE: &str = "auto_backup_ms";
const TRIED: &str = "auto_backup_tried_ms";
const FAILED: &str = "auto_backup_error";

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
pub const EVERY_MS: i64 = 7 * DAY_MS;
/// After a failure (a drive that is not plugged in), the next try waits.
pub const RETRY_MS: i64 = DAY_MS / 4;
pub const KEEP: usize = 3;

const PREFIX: &str = "Unstrung auto backup ";
const SUFFIX: &str = ".zip";

#[derive(Debug, Clone, Serialize)]
pub struct AutoBackup {
    /// `None` when automatic backups are off.
    pub folder: Option<PathBuf>,
    pub last_ms: Option<i64>,
    /// The reason the last try failed, until one works.
    pub error: Option<String>,
}

async fn text(db: &Db, key: &str) -> Result<Option<String>> {
    Ok(db.meta(key).await?.filter(|v| !v.is_empty()))
}

async fn ms(db: &Db, key: &str) -> Result<Option<i64>> {
    Ok(text(db, key).await?.and_then(|v| v.parse().ok()))
}

pub async fn status(db: &Db) -> Result<AutoBackup> {
    Ok(AutoBackup {
        folder: text(db, FOLDER).await?.map(PathBuf::from),
        last_ms: ms(db, DONE).await?,
        error: text(db, FAILED).await?,
    })
}

/// Turns automatic backups on with `folder`, or off with `None`. The next
/// hourly check makes the first backup when the last one is a week old.
pub async fn set_folder(db: &Db, paths: &AppPaths, folder: Option<&Path>) -> Result<AutoBackup> {
    if let Some(folder) = folder
        && crate::relocate::inside(folder, paths.root())
    {
        // An uninstall deletes the app folder, with the backups in it.
        return Err(Error::Invalid(
            "pick a folder outside the Unstrung app folder".into(),
        ));
    }
    let value = folder.map(|f| f.to_string_lossy().into_owned());
    db.set_meta(FOLDER, value.as_deref().unwrap_or_default())
        .await?;
    db.set_meta(FAILED, "").await?;
    db.set_meta(TRIED, "").await?;
    status(db).await
}

/// Makes the weekly backup when it is due. `Ok(None)` when it is off or
/// not due yet. A failure is saved for Settings, and the next try waits.
pub async fn run_if_due(
    db: &Db,
    paths: &AppPaths,
    now: i64,
    app_version: &str,
) -> Result<Option<Exported>> {
    let Some(folder) = text(db, FOLDER).await?.map(PathBuf::from) else {
        return Ok(None);
    };
    if ms(db, DONE).await?.is_some_and(|t| now - t < EVERY_MS) {
        return Ok(None);
    }
    if text(db, FAILED).await?.is_some() && ms(db, TRIED).await?.is_some_and(|t| now - t < RETRY_MS)
    {
        return Ok(None);
    }
    db.set_meta(TRIED, &now.to_string()).await?;
    let made = if folder.is_dir() {
        backup::export(db, paths, &folder.join(file_name(now)), app_version).await
    } else {
        Err(Error::Invalid(format!(
            "the folder {} is not there. Plug in its drive, or pick another folder",
            folder.display()
        )))
    };
    match made {
        Ok(done) => {
            db.set_meta(DONE, &now.to_string()).await?;
            db.set_meta(FAILED, "").await?;
            prune(&folder);
            Ok(Some(done))
        }
        Err(e) => {
            // Settings shows this text as it is.
            let why = match &e {
                Error::Invalid(text) => text.clone(),
                Error::NoSpace { need } => format!(
                    "the drive needs {} MB more free space",
                    need.div_ceil(1024 * 1024)
                ),
                other => other.to_string(),
            };
            db.set_meta(FAILED, &why).await?;
            Err(e)
        }
    }
}

/// "Unstrung auto backup 2026-11-09.zip". One a day at most, and the names
/// sort by date.
fn file_name(ms: i64) -> String {
    let (y, m, d) = date(ms);
    format!("{PREFIX}{y:04}-{m:02}-{d:02}{SUFFIX}")
}

/// The UTC date of a time in ms (Howard Hinnant's days-to-civil algorithm).
fn date(ms: i64) -> (i64, i64, i64) {
    let z = ms.div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// True for a name that `file_name` makes.
fn ours(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
        .is_some_and(|d| {
            d.len() == 10
                && d.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
        })
}

/// Keeps the `KEEP` newest backups and removes half-written ones. Files with
/// any other name stay.
fn prune(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut backups = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if ours(&name) {
            backups.push(name);
        } else if name.strip_suffix(".part").is_some_and(ours) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    backups.sort_unstable_by(|a, b| b.cmp(a));
    for old in backups.iter().skip(KEEP) {
        if let Err(e) = std::fs::remove_file(folder.join(old)) {
            log::warn!("could not remove the old automatic backup {old}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_carry_the_date_and_only_ours_match() {
        assert_eq!(date(0), (1970, 1, 1));
        // 2026-11-09 12:00 UTC.
        assert_eq!(date(1_794_225_600_000), (2026, 11, 9));
        assert_eq!(
            file_name(1_794_225_600_000),
            "Unstrung auto backup 2026-11-09.zip"
        );
        assert!(ours("Unstrung auto backup 2026-11-09.zip"));
        for other in [
            "Unstrung backup.zip",
            "Unstrung auto backup 2026-11-9.zip",
            "Unstrung auto backup 2026-11-09 (1).zip",
            "unstrung auto backup 2026-11-09.zip",
        ] {
            assert!(!ours(other), "{other}");
        }
    }

    #[test]
    fn prune_keeps_the_newest_three_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let days = ["01", "02", "03", "04", "05"];
        for d in days {
            std::fs::write(
                dir.path().join(format!("{PREFIX}2026-11-{d}{SUFFIX}")),
                b"z",
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join(format!("{PREFIX}2026-10-01.zip.part")),
            b"z",
        )
        .unwrap();
        std::fs::write(dir.path().join("my notes.zip"), b"z").unwrap();
        prune(dir.path());
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "Unstrung auto backup 2026-11-03.zip",
                "Unstrung auto backup 2026-11-04.zip",
                "Unstrung auto backup 2026-11-05.zip",
                "my notes.zip",
            ]
        );
    }

    #[tokio::test]
    async fn a_backup_comes_once_a_week_and_a_failure_waits() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let db = Db::open(&paths.database(), &paths.key().unwrap())
            .await
            .unwrap();
        let start = crate::db::now_ms();
        assert!(
            run_if_due(&db, &paths, start, "1").await.unwrap().is_none(),
            "off"
        );

        assert!(
            set_folder(&db, &paths, Some(&paths.root().join("b")))
                .await
                .is_err(),
            "never inside the app folder"
        );
        let out = dir.path().join("out");
        set_folder(&db, &paths, Some(&out)).await.unwrap();
        // The folder is missing: the failure is saved, and the next hour waits.
        assert!(run_if_due(&db, &paths, start, "1").await.is_err());
        assert!(status(&db).await.unwrap().error.is_some());
        let hour = 60 * 60 * 1000;
        assert!(
            run_if_due(&db, &paths, start + hour, "1")
                .await
                .unwrap()
                .is_none()
        );

        std::fs::create_dir_all(&out).unwrap();
        let later = start + RETRY_MS;
        assert!(run_if_due(&db, &paths, later, "1").await.unwrap().is_some());
        assert!(out.join(file_name(later)).is_file());
        let now = status(&db).await.unwrap();
        assert_eq!((now.last_ms, now.error), (Some(later), None));
        assert!(
            run_if_due(&db, &paths, later + 6 * DAY_MS, "1")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            run_if_due(&db, &paths, later + EVERY_MS, "1")
                .await
                .unwrap()
                .is_some()
        );

        set_folder(&db, &paths, None).await.unwrap();
        assert!(status(&db).await.unwrap().folder.is_none());
    }
}
