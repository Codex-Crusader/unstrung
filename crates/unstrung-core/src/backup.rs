//! "Export everything" and "Import backup". A backup is a zip with a
//! manifest, a clean copy of the chats file, the avatars and the gallery
//! pictures. Engines, models, logs and files about this PC (such as
//! `location.json`) stay out. Every setting is a row in the chats file.

use crate::db::{Db, now_ms};
use crate::error::{Error, Result};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

pub const MANIFEST: &str = "unstrung-backup.json";
pub const DATABASE: &str = "app.db";
const FORMAT: u32 = 1;
/// The only files besides the chats file and the manifest.
const PICTURE_TYPES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// Work files in `backups` that a crash can leave behind.
const LEFTOVER_PREFIXES: [&str; 1] = ["export-"];
/// Room that stays free on the disk after an export.
const SPARE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format: u32,
    /// The highest migration of the chats file.
    pub schema: i64,
    pub app_version: String,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    pub bytes: u64,
    pub avatars: usize,
    pub pictures: usize,
}

fn zip_error(e: zip::result::ZipError) -> Error {
    Error::Io(std::io::Error::other(e))
}

fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PICTURE_TYPES.contains(&e.to_ascii_lowercase().as_str()))
}

/// The picture files under `dir`, with their names in the zip
/// (`prefix/a/b.png`). Regular files only: links and junctions are not
/// followed, so nothing outside the folder goes in.
fn pictures(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        let Ok(name) = entry.file_name().into_string() else {
            log::warn!(
                "the backup skips a file name it cannot read in {}",
                dir.display()
            );
            continue;
        };
        let inner = format!("{prefix}/{name}");
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            pictures(&path, &inner, out)?;
        } else if kind.is_file() && is_picture(&path) {
            out.push((path, inner));
        }
    }
    Ok(())
}

/// Adds one file. `false` when the file went away meanwhile (a picture
/// deleted during the export).
fn add(
    zip: &mut ZipWriter<BufWriter<File>>,
    file: &Path,
    name: &str,
    method: CompressionMethod,
) -> Result<bool> {
    let mut source = match File::open(file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let len = source.metadata()?.len();
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(len >= u64::from(u32::MAX));
    zip.start_file(name, options).map_err(zip_error)?;
    std::io::copy(&mut source, zip)?;
    Ok(true)
}

fn write_zip(paths: &AppPaths, db_copy: &Path, to: &Path, manifest: &Manifest) -> Result<Exported> {
    let mut files = Vec::new();
    pictures(&paths.avatars(), "avatars", &mut files)?;
    let avatar_count = files.len();
    pictures(&paths.gallery(), "gallery", &mut files)?;
    let size = |f: &Path| std::fs::metadata(f).map_or(0, |m| m.len());
    let need = size(db_copy) + files.iter().map(|(f, _)| size(f)).sum::<u64>() + SPARE_BYTES;
    if let Some(disk) = crate::hardware::disk_of(to)
        && disk.available < need
    {
        return Err(Error::NoSpace {
            need: need - disk.available,
        });
    }
    let part = PathBuf::from(format!("{}.part", to.display()));
    let written = (|| -> Result<Exported> {
        let mut zip = ZipWriter::new(BufWriter::new(File::create(&part)?));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(MANIFEST, deflated).map_err(zip_error)?;
        zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;
        add(&mut zip, db_copy, DATABASE, CompressionMethod::Deflated)?;
        let (mut avatars, mut pictures) = (0, 0);
        for (i, (file, name)) in files.iter().enumerate() {
            // Pictures are compressed already.
            if add(&mut zip, file, name, CompressionMethod::Stored)? {
                *(if i < avatar_count {
                    &mut avatars
                } else {
                    &mut pictures
                }) += 1;
            }
        }
        let out = zip.finish().map_err(zip_error)?;
        out.into_inner()
            .map_err(|e| Error::Io(e.into_error()))?
            .sync_all()?;
        Ok(Exported {
            bytes: size(&part),
            avatars,
            pictures,
        })
    })();
    match written {
        Ok(done) => {
            std::fs::rename(&part, to)?;
            Ok(done)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Writes a backup of everything to `to`, through a `.part` file, so a
/// failed export leaves no zip that looks complete.
pub async fn export(db: &Db, paths: &AppPaths, to: &Path, app_version: &str) -> Result<Exported> {
    let manifest = Manifest {
        format: FORMAT,
        schema: db
            .meta("schema_version")
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        app_version: app_version.to_owned(),
        created_ms: now_ms(),
    };
    std::fs::create_dir_all(paths.backups())?;
    // A clean copy: never the live file, which may have changes in its journal.
    let copy = paths
        .backups()
        .join(format!("{}{}.db", LEFTOVER_PREFIXES[0], now_ms()));
    db.copy_to(&copy).await?;
    let (p, c, t) = (paths.clone(), copy.clone(), to.to_path_buf());
    let result = tokio::task::spawn_blocking(move || write_zip(&p, &c, &t, &manifest))
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)));
    if let Err(e) = std::fs::remove_file(&copy) {
        log::warn!("could not delete {}: {e}", copy.display());
    }
    result?
}

/// Removes the work files of an export or import that a crash left behind.
pub fn clear_leftovers(paths: &AppPaths) {
    let Ok(entries) = std::fs::read_dir(paths.backups()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if LEFTOVER_PREFIXES.iter().any(|p| name.starts_with(p))
            && entry.file_type().is_ok_and(|t| t.is_file())
            && let Err(e) = std::fs::remove_file(entry.path())
        {
            log::warn!("could not delete the old work file {name}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn an_export_holds_the_chats_file_and_the_pictures_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();
        std::fs::write(paths.avatars().join("a.png"), b"avatar").unwrap();
        std::fs::create_dir_all(paths.gallery().join("1")).unwrap();
        std::fs::write(paths.gallery().join("1").join("p.PNG"), b"picture").unwrap();
        std::fs::write(paths.gallery().join("1").join("notes.txt"), b"x").unwrap();
        std::fs::write(paths.logs().join("app.log"), b"log").unwrap();
        let to = dir.path().join("backup.zip");

        let done = export(&db, &paths, &to, "1.2.3").await.unwrap();
        assert_eq!((done.avatars, done.pictures), (1, 1));
        let mut zip = zip::ZipArchive::new(File::open(&to).unwrap()).unwrap();
        let names: BTreeSet<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        let expected = [MANIFEST, DATABASE, "avatars/a.png", "gallery/1/p.PNG"];
        assert_eq!(names, expected.map(String::from).into());
        let manifest: Manifest = serde_json::from_reader(zip.by_name(MANIFEST).unwrap()).unwrap();
        assert_eq!(manifest.format, FORMAT);
        assert_eq!(manifest.app_version, "1.2.3");
        let newest = db.meta("schema_version").await.unwrap().unwrap();
        assert_eq!(manifest.schema.to_string(), newest);
        // No work files stay behind.
        assert!(!dir.path().join("backup.zip.part").exists());
        let left: Vec<_> = std::fs::read_dir(paths.backups())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("export-"))
            .collect();
        assert!(left.is_empty());
    }
}
