//! Writes a backup zip and checks it after writing.

use super::*;

pub(super) fn zip_error(e: zip::result::ZipError) -> Error {
    Error::Io(std::io::Error::other(e))
}

pub(super) fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PICTURE_TYPES.contains(&e.to_ascii_lowercase().as_str()))
}

/// The picture files under `dir`, with their names in the zip
/// (`prefix/a/b.png`). Regular files only: links and junctions are not
/// followed, so nothing outside the folder goes in.
pub(super) fn pictures(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
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

/// Adds one picture, decrypted: a backup is how data moves to a new PC, so
/// it must open without this PC's key. `false` when the file went away
/// meanwhile (a picture deleted during the export).
pub(super) fn add_picture(
    zip: &mut ZipWriter<BufWriter<File>>,
    key: &crate::vault::DataKey,
    file: &Path,
    name: &str,
    sealed: Option<&BackupKey>,
) -> Result<bool> {
    let bytes = match std::fs::read(file) {
        Ok(b) => key.open(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    // Pictures are compressed already.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(name, options).map_err(zip_error)?;
    match sealed {
        Some(backup) => backup.seal(name, &mut bytes.as_slice(), zip)?,
        None => zip.write_all(&bytes)?,
    }
    Ok(true)
}

/// Reads a written backup back before it counts: every file must be in it,
/// and each one is read to its end, which checks its CRC.
pub(super) fn check_zip(file: &Path, files: usize, key: Option<&BackupKey>) -> Result<()> {
    let failed = |why: String| {
        Error::Invalid(format!(
            "the backup did not pass its check after writing ({why}). Try again, or pick another folder"
        ))
    };
    let mut zip = ZipArchive::new(File::open(file)?).map_err(|e| failed(e.to_string()))?;
    if zip.len() != files {
        return Err(failed(format!("{} of {files} files", zip.len())));
    }
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| failed(e.to_string()))?;
        std::io::copy(&mut entry, &mut std::io::sink()).map_err(|e| failed(e.to_string()))?;
    }
    if let Some(key) = key {
        // The manifest must hold the check that this key made.
        let mut bytes = Vec::new();
        zip.by_name(MANIFEST)
            .map_err(|e| failed(e.to_string()))?
            .read_to_end(&mut bytes)?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        manifest
            .encryption
            .ok_or_else(|| failed("no password check".into()))
            .and_then(|e| {
                key.verify(&e)
                    .map_err(|_| failed("the password check".into()))
            })?;
    }
    Ok(())
}

/// Adds one file, encrypted when there is a `sealed` key.
pub(super) fn add(
    zip: &mut ZipWriter<BufWriter<File>>,
    file: &Path,
    name: &str,
    method: CompressionMethod,
    sealed: Option<&BackupKey>,
) -> Result<bool> {
    let mut source = match File::open(file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let len = source.metadata()?.len();
    // Encrypted bytes do not compress.
    let method = if sealed.is_some() {
        CompressionMethod::Stored
    } else {
        method
    };
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(len >= u64::from(u32::MAX) - (1 << 20));
    zip.start_file(name, options).map_err(zip_error)?;
    match sealed {
        Some(key) => key.seal(name, &mut source, zip)?,
        None => {
            std::io::copy(&mut source, zip)?;
        }
    }
    Ok(true)
}

pub(super) fn write_zip(
    paths: &AppPaths,
    db_copy: &Path,
    to: &Path,
    mut manifest: Manifest,
    password: Option<&str>,
) -> Result<Exported> {
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
    let key = paths.key()?;
    // One key for the whole export: making it is slow on purpose.
    let sealed = match password {
        Some(p) => {
            let (backup, encryption) = BackupKey::new(p)?;
            manifest.format = FORMAT_SEALED;
            manifest.encryption = Some(encryption);
            Some(backup)
        }
        None => None,
    };
    let sealed = sealed.as_ref();
    let part = PathBuf::from(format!("{}.part", to.display()));
    let written = (|| -> Result<Exported> {
        let mut zip = ZipWriter::new(BufWriter::new(File::create(&part)?));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(MANIFEST, deflated).map_err(zip_error)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        add(
            &mut zip,
            db_copy,
            DATABASE,
            CompressionMethod::Deflated,
            sealed,
        )?;
        let (mut avatars, mut pictures) = (0, 0);
        for (i, (file, name)) in files.iter().enumerate() {
            if add_picture(&mut zip, &key, file, name, sealed)? {
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
        check_zip(&part, 2 + avatars + pictures, sealed)?;
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
        encryption: None,
    };
    let password = password(db).await?;
    std::fs::create_dir_all(paths.backups())?;
    // A clean copy: never the live file, which may have changes in its journal.
    let copy = paths
        .backups()
        .join(format!("{}{}.db", LEFTOVER_PREFIXES[0], now_ms()));
    db.plain_copy_to(&copy).await?;
    if let Err(e) = crate::autobackup::forget(&copy).await {
        let _ = std::fs::remove_file(&copy);
        return Err(e);
    }
    let (p, c, t) = (paths.clone(), copy.clone(), to.to_path_buf());
    let result =
        tokio::task::spawn_blocking(move || write_zip(&p, &c, &t, manifest, password.as_deref()))
            .await
            .map_err(|e| Error::Io(std::io::Error::other(e)));
    if let Err(e) = std::fs::remove_file(&copy) {
        log::warn!("could not delete {}: {e}", copy.display());
    }
    let done = result??;
    // The zip is safe on disk; a failed note only brings the reminder early.
    if let Err(e) = db.set_meta(LAST_BACKUP, &now_ms().to_string()).await {
        log::warn!("could not note the backup time: {e}");
    }
    Ok(done)
}
