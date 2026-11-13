//! Opens a backup zip into a staging folder and checks what it holds.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Manifest,
    Database,
    Avatar,
    Gallery,
}

/// What a name in the zip is. `Ok(None)`: an unknown file, skipped.
/// A wrong name under `avatars/` or `gallery/` refuses the backup.
pub(super) fn part_of(name: &str) -> std::result::Result<Option<Part>, String> {
    let part = if name == MANIFEST {
        Part::Manifest
    } else if name == DATABASE {
        Part::Database
    } else if let Some(file) = name.strip_prefix("avatars/") {
        // The app saves avatars as PNG files right in the folder.
        if file.contains(['/', '\\']) || !file.to_ascii_lowercase().ends_with(".png") {
            return Err(format!("unexpected file {name}"));
        }
        Part::Avatar
    } else if name.starts_with("gallery/") {
        if !is_picture(Path::new(name)) {
            return Err(format!("unexpected file {name}"));
        }
        Part::Gallery
    } else {
        return Ok(None);
    };
    // The zip crate's own name check misses device names and `:`.
    join_safe(Path::new("check"), name).map_err(|_| format!("unsafe file name {name}"))?;
    Ok(Some(part))
}

pub(super) fn not_a_backup(why: &str) -> Error {
    Error::Invalid(format!(
        "this file is not a backup that Unstrung can read: {why}"
    ))
}

pub(super) fn newer_version() -> Error {
    Error::Invalid(
        "this backup comes from a newer version of Unstrung. Update Unstrung first".into(),
    )
}

/// The staging folders of import `id`: the chats file and avatars, and the pictures.
pub(super) fn staging(paths: &AppPaths, id: &str) -> Result<(PathBuf, PathBuf)> {
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid("this backup is no longer open".into()));
    }
    Ok((
        paths.backups().join(format!("{IMPORT_PREFIX}{id}")),
        paths.library().join(format!("{GALLERY_IMPORT_PREFIX}{id}")),
    ))
}

/// Removes the staged files of import `id`.
pub fn discard(paths: &AppPaths, id: &str) {
    let Ok((dir, gallery)) = staging(paths, id) else {
        return;
    };
    for d in [dir, gallery] {
        if d.exists()
            && let Err(e) = std::fs::remove_dir_all(&d)
        {
            log::warn!("could not remove {}: {e}", d.display());
        }
    }
}

pub(super) fn has_room(dir: &Path, bytes: u64) -> Result<()> {
    if let Some(disk) = crate::hardware::disk_of(dir)
        && disk.available < bytes + IMPORT_SPARE_BYTES
    {
        return Err(Error::NoSpace {
            need: bytes + IMPORT_SPARE_BYTES - disk.available,
        });
    }
    Ok(())
}

/// Reads exactly `size` bytes of an entry. More or fewer: the header lied
/// or the file is damaged. The zip reader checks the CRC at the end.
pub(super) fn read_exact_size(
    entry: &mut impl Read,
    size: u64,
    out: &mut impl Write,
) -> Result<()> {
    let copied = std::io::copy(&mut entry.take(size + 1), out)
        .map_err(|e| not_a_backup(&format!("a file is damaged ({e})")))?;
    if copied != size {
        return Err(not_a_backup("a file is damaged (wrong size)"));
    }
    Ok(())
}

/// Checks every name and size in the zip, then unpacks the backup into the
/// staging folders of `id`. Avatars are decoded and saved again.
pub(super) fn unpack(
    paths: &AppPaths,
    zip_path: &Path,
    id: &str,
    password: Option<&str>,
) -> Result<Manifest> {
    let mut zip =
        ZipArchive::new(File::open(zip_path)?).map_err(|e| not_a_backup(&e.to_string()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(not_a_backup("it holds too many files"));
    }
    let mut plan: Vec<(usize, Part, String, u64)> = Vec::new();
    let mut seen = HashSet::new();
    let (mut total, mut pictures) = (0u64, 0u64);
    for i in 0..zip.len() {
        let entry = zip
            .by_index_raw(i)
            .map_err(|e| not_a_backup(&e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|e| not_a_backup(&e.to_string()))?
            .into_owned();
        let Some(part) = part_of(&name).map_err(|why| not_a_backup(&why))? else {
            log::info!("the backup import skips an unknown file: {name}");
            continue;
        };
        if entry.is_symlink() {
            return Err(not_a_backup(&format!("{name} is a link")));
        }
        // Windows does not tell `A.png` from `a.png`: one would overwrite the other.
        if !seen.insert(name.to_lowercase()) {
            return Err(not_a_backup(&format!("{name} is in it twice")));
        }
        let size = entry.size();
        let cap = match part {
            Part::Manifest => MAX_MANIFEST_BYTES,
            Part::Database => MAX_DATABASE_BYTES,
            Part::Avatar | Part::Gallery => MAX_PICTURE_BYTES,
        };
        if size > cap || (size > 1 << 20 && size / entry.compressed_size().max(1) > MAX_RATIO) {
            return Err(not_a_backup(&format!("{name} is too large")));
        }
        total += size;
        if part == Part::Gallery {
            pictures += size;
        }
        plan.push((i, part, name, size));
    }
    if total > MAX_TOTAL_BYTES {
        return Err(not_a_backup("it is too large"));
    }
    for needed in [Part::Manifest, Part::Database] {
        if !plan.iter().any(|p| p.1 == needed) {
            return Err(not_a_backup(
                "the chats file or the backup notes are missing",
            ));
        }
    }
    // The notes first: they say whether the files need a password.
    let manifest = {
        let (i, size) = plan
            .iter()
            .find(|p| p.1 == Part::Manifest)
            .map(|p| (p.0, p.3))
            .ok_or_else(|| not_a_backup("the backup notes are missing"))?;
        let mut entry = zip.by_index(i).map_err(|e| not_a_backup(&e.to_string()))?;
        let mut bytes = Vec::new();
        read_exact_size(&mut entry, size, &mut bytes)?;
        serde_json::from_slice::<Manifest>(&bytes)
            .map_err(|_| not_a_backup("the backup notes cannot be read"))?
    };
    if manifest.format > FORMAT_SEALED {
        return Err(newer_version());
    }
    let sealed = match (&manifest.encryption, password) {
        (None, _) => None,
        (Some(_), None) => return Err(Error::PasswordNeeded),
        (Some(e), Some(p)) => Some(BackupKey::unlock(p, e)?),
    };
    // A file of an encrypted backup comes out of `entry` decrypted.
    let read = |entry: &mut zip::read::ZipFile<'_, File>,
                name: &str,
                size: u64,
                out: &mut dyn Write|
     -> Result<()> {
        match &sealed {
            Some(backup) => backup.open(name, entry, size, &mut &mut *out),
            None => read_exact_size(entry, size, &mut &mut *out),
        }
    };
    has_room(&paths.backups(), total)?;
    has_room(&paths.library(), pictures)?;
    let (dir, gallery) = staging(paths, id)?;
    let key = paths.key()?;
    let unpacked = (|| -> Result<Manifest> {
        std::fs::create_dir_all(dir.join("avatars"))?;
        std::fs::create_dir_all(&gallery)?;
        for (i, part, name, size) in &plan {
            if *part == Part::Manifest {
                continue;
            }
            let mut entry = zip.by_index(*i).map_err(|e| not_a_backup(&e.to_string()))?;
            if *part == Part::Database {
                let mut out = File::create(dir.join(DATABASE))?;
                read(&mut entry, name, *size, &mut out)?;
                out.sync_all()?;
                continue;
            }
            let mut bytes = Vec::new();
            read(&mut entry, name, *size, &mut bytes)?;
            match part {
                Part::Avatar => {
                    let clean = crate::avatar::clean_copy(&bytes)
                        .map_err(|e| not_a_backup(&format!("{name}: {e}")))?;
                    key.write(&join_safe(&dir, name)?, &clean)?;
                }
                _ => {
                    let kind = image::guess_format(&bytes).ok();
                    if !matches!(
                        kind,
                        Some(
                            image::ImageFormat::Png
                                | image::ImageFormat::Jpeg
                                | image::ImageFormat::WebP
                        )
                    ) {
                        return Err(not_a_backup(&format!("{name} is not a picture")));
                    }
                    let rel = name.strip_prefix("gallery/").unwrap_or(name);
                    let to = join_safe(&gallery, rel)?;
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    key.write(&to, &bytes)?;
                }
            }
        }
        Ok(manifest)
    })();
    if unpacked.is_err() {
        discard(paths, id);
    }
    unpacked
}

/// What a backup holds, shown before the user picks Replace or Add.
#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    /// Names the staged files for the next step.
    pub id: String,
    pub created_ms: i64,
    pub app_version: String,
    pub characters: i64,
    pub personas: i64,
    pub chats: i64,
    pub pictures: i64,
}

pub(super) fn character_fields(c: &Character) -> CharacterFields {
    CharacterFields {
        name: c.name.clone(),
        tagline: c.tagline.clone(),
        personality: c.personality.clone(),
        scenario: c.scenario.clone(),
        greeting: c.greeting.clone(),
        example_dialogue: c.example_dialogue.clone(),
        voice_anchor: c.voice_anchor.clone(),
        appearance_tags: c.appearance_tags.clone(),
        outfit: c.outfit.clone(),
        default_tone: c.default_tone.clone(),
        default_reply_style: c.default_reply_style.clone(),
        default_reply_custom: c.default_reply_custom.clone(),
        art_style: c.art_style.clone(),
        age: c.age,
        is_adult: c.is_adult,
    }
}

pub(super) fn persona_fields(p: &Persona) -> PersonaFields {
    PersonaFields {
        name: p.name.clone(),
        description: p.description.clone(),
        appearance_tags: p.appearance_tags.clone(),
        outfit: p.outfit.clone(),
        age: p.age,
    }
}

/// An avatar path that the app could have saved: `avatars/<file>.png`.
pub(super) fn avatar_ok(path: Option<&str>) -> bool {
    path.is_none_or(|p| {
        p.strip_prefix("avatars/")
            .is_some_and(|f| !f.contains(['/', '\\']))
            && join_safe(Path::new("check"), p).is_ok()
    })
}

/// The rows of the staged chats file go through the same checks as the
/// creator: the same lengths, presets and ages. A bad row refuses the backup.
pub(super) async fn check_rows(db: &Db) -> Result<[i64; 4]> {
    let bad = |what: &str, name: &str, why: &dyn std::fmt::Display| {
        not_a_backup(&format!("{what} {name} fails a check ({why})"))
    };
    for c in db.characters().await? {
        crate::library::check_character(&character_fields(&c))
            .map_err(|e| bad("the character", &c.name, &e))?;
        if !avatar_ok(c.avatar_path.as_deref()) {
            return Err(bad("the character", &c.name, &"picture path"));
        }
    }
    for p in db.personas().await? {
        crate::library::check_persona(&persona_fields(&p))
            .map_err(|e| bad("the persona", &p.name, &e))?;
        if !avatar_ok(p.avatar_path.as_deref()) {
            return Err(bad("the persona", &p.name, &"picture path"));
        }
    }
    for path in db.image_paths().await? {
        join_safe(Path::new("check"), &path).map_err(|_| bad("the picture", &path, &"path"))?;
    }
    db.counts().await
}

pub(super) async fn check_staged(
    paths: &AppPaths,
    id: &str,
    manifest: Manifest,
) -> Result<BackupInfo> {
    let (dir, _) = staging(paths, id)?;
    let file = dir.join(DATABASE);
    // The chats file comes plain out of the zip. It stays here until the
    // user restores or closes the backup, so it is encrypted at once.
    if crate::vault::is_plain_database(&file)
        && crate::db::encrypt_plain(&file, &paths.key()?)
            .await
            .is_err()
    {
        return Err(not_a_backup("the chats file in it is damaged"));
    }
    match Db::integrity_check_file(&file, &paths.key()?).await {
        Ok(Some(problems)) if problems.is_empty() => {}
        _ => return Err(not_a_backup("the chats file in it is damaged")),
    }
    let key = paths.key()?;
    let schema = Db::schema_of_file(&file, &key)
        .await
        .map_err(|_| not_a_backup("it holds no Unstrung chats file"))?;
    if schema > crate::db::newest_schema() {
        return Err(newer_version());
    }
    // Brings an older backup up to this version, in the staging folder.
    let db = Db::open(&file, &key).await?;
    // The 18+ confirmation is for this install, not something a file brings.
    let counts = match crate::content::set_adult(&db, false).await {
        Ok(()) => check_rows(&db).await,
        Err(e) => Err(e),
    };
    db.close().await;
    let [characters, personas, chats, pictures] = counts?;
    Ok(BackupInfo {
        id: id.to_owned(),
        created_ms: manifest.created_ms,
        app_version: manifest.app_version,
        characters,
        personas,
        chats,
        pictures,
    })
}

/// Opens a backup zip: checks it, unpacks it into staging folders and
/// checks the chats file in it. Nothing of the current data changes.
pub async fn open(paths: &AppPaths, zip_path: &Path) -> Result<BackupInfo> {
    open_with(paths, zip_path, None).await
}

/// `open` for a backup that may have a password. Without one, an encrypted
/// backup gives `Error::PasswordNeeded`.
pub async fn open_with(
    paths: &AppPaths,
    zip_path: &Path,
    password: Option<String>,
) -> Result<BackupInfo> {
    let id = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    std::fs::create_dir_all(paths.backups())?;
    let (p, z, i) = (paths.clone(), zip_path.to_path_buf(), id.clone());
    let manifest = tokio::task::spawn_blocking(move || unpack(&p, &z, &i, password.as_deref()))
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)))??;
    let checked = check_staged(paths, &id, manifest).await;
    if checked.is_err() {
        discard(paths, &id);
    }
    checked
}
