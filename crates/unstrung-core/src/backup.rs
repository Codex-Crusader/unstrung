//! "Export everything" and "Import backup". A backup is a zip with a
//! manifest, a clean copy of the chats file, the avatars and the gallery
//! pictures. Engines, models, logs and files about this PC (such as
//! `location.json`) stay out. Every setting is a row in the chats file.

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

pub const MANIFEST: &str = "unstrung-backup.json";
pub const DATABASE: &str = "app.db";
const FORMAT: u32 = 1;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Manifest,
    Database,
    Avatar,
    Gallery,
}

/// What a name in the zip is. `Ok(None)`: an unknown file, skipped.
/// A wrong name under `avatars/` or `gallery/` refuses the backup.
fn part_of(name: &str) -> std::result::Result<Option<Part>, String> {
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

fn not_a_backup(why: &str) -> Error {
    Error::Invalid(format!(
        "this file is not a backup that Unstrung can read: {why}"
    ))
}

fn newer_version() -> Error {
    Error::Invalid(
        "this backup comes from a newer version of Unstrung. Update Unstrung first".into(),
    )
}

/// The staging folders of import `id`: the chats file and avatars, and the pictures.
fn staging(paths: &AppPaths, id: &str) -> Result<(PathBuf, PathBuf)> {
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

fn has_room(dir: &Path, bytes: u64) -> Result<()> {
    if let Some(disk) = crate::hardware::disk_of(dir)
        && disk.available < bytes + SPARE_BYTES
    {
        return Err(Error::NoSpace {
            need: bytes + SPARE_BYTES - disk.available,
        });
    }
    Ok(())
}

/// Reads exactly `size` bytes of an entry. More or fewer: the header lied
/// or the file is damaged. The zip reader checks the CRC at the end.
fn read_exact_size(entry: &mut impl Read, size: u64, out: &mut impl Write) -> Result<()> {
    let copied = std::io::copy(&mut entry.take(size + 1), out)
        .map_err(|e| not_a_backup(&format!("a file is damaged ({e})")))?;
    if copied != size {
        return Err(not_a_backup("a file is damaged (wrong size)"));
    }
    Ok(())
}

/// Checks every name and size in the zip, then unpacks the backup into the
/// staging folders of `id`. Avatars are decoded and saved again.
fn unpack(paths: &AppPaths, zip_path: &Path, id: &str) -> Result<Manifest> {
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
    has_room(&paths.backups(), total)?;
    has_room(&paths.library(), pictures)?;
    let (dir, gallery) = staging(paths, id)?;
    let unpacked = (|| -> Result<Manifest> {
        std::fs::create_dir_all(dir.join("avatars"))?;
        std::fs::create_dir_all(&gallery)?;
        let mut manifest = None;
        for (i, part, name, size) in &plan {
            let mut entry = zip.by_index(*i).map_err(|e| not_a_backup(&e.to_string()))?;
            if *part == Part::Database {
                let mut out = File::create(dir.join(DATABASE))?;
                read_exact_size(&mut entry, *size, &mut out)?;
                out.sync_all()?;
                continue;
            }
            let mut bytes = Vec::new();
            read_exact_size(&mut entry, *size, &mut bytes)?;
            match part {
                Part::Manifest => {
                    manifest = Some(
                        serde_json::from_slice::<Manifest>(&bytes)
                            .map_err(|_| not_a_backup("the backup notes cannot be read"))?,
                    );
                }
                Part::Avatar => {
                    let clean = crate::avatar::clean_copy(&bytes)
                        .map_err(|e| not_a_backup(&format!("{name}: {e}")))?;
                    std::fs::write(join_safe(&dir, name)?, clean)?;
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
                    std::fs::write(to, &bytes)?;
                }
            }
        }
        let manifest = manifest.ok_or_else(|| not_a_backup("the backup notes are missing"))?;
        if manifest.format > FORMAT {
            return Err(newer_version());
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

fn character_fields(c: &Character) -> CharacterFields {
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

fn persona_fields(p: &Persona) -> PersonaFields {
    PersonaFields {
        name: p.name.clone(),
        description: p.description.clone(),
        appearance_tags: p.appearance_tags.clone(),
        outfit: p.outfit.clone(),
        age: p.age,
    }
}

/// An avatar path that the app could have saved: `avatars/<file>.png`.
fn avatar_ok(path: Option<&str>) -> bool {
    path.is_none_or(|p| {
        p.strip_prefix("avatars/")
            .is_some_and(|f| !f.contains(['/', '\\']))
            && join_safe(Path::new("check"), p).is_ok()
    })
}

/// The rows of the staged chats file go through the same checks as the
/// creator: the same lengths, presets and ages. A bad row refuses the backup.
async fn check_rows(db: &Db) -> Result<[i64; 4]> {
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

async fn check_staged(paths: &AppPaths, id: &str, manifest: Manifest) -> Result<BackupInfo> {
    let (dir, _) = staging(paths, id)?;
    let file = dir.join(DATABASE);
    match Db::integrity_check_file(&file).await {
        Ok(Some(problems)) if problems.is_empty() => {}
        _ => return Err(not_a_backup("the chats file in it is damaged")),
    }
    let schema = Db::schema_of_file(&file)
        .await
        .map_err(|_| not_a_backup("it holds no Unstrung chats file"))?;
    if schema > crate::db::newest_schema() {
        return Err(newer_version());
    }
    // Brings an older backup up to this version, in the staging folder.
    let db = Db::open(&file).await?;
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
    let id = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    std::fs::create_dir_all(paths.backups())?;
    let (p, z, i) = (paths.clone(), zip_path.to_path_buf(), id.clone());
    let manifest = tokio::task::spawn_blocking(move || unpack(&p, &z, &i))
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)))??;
    let checked = check_staged(paths, &id, manifest).await;
    if checked.is_err() {
        discard(paths, &id);
    }
    checked
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Added {
    pub characters: usize,
    pub personas: usize,
    /// Already here with the same text, so not added again (the starters).
    pub skipped: usize,
}

/// Copies a staged avatar (a clean PNG already) into the avatars folder
/// under a new name. `None` when the backup has no such file.
fn copy_avatar(
    paths: &AppPaths,
    staged: &Path,
    stored: Option<&str>,
    n: usize,
) -> Result<Option<String>> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    let Ok(bytes) = std::fs::read(join_safe(staged, stored)?) else {
        return Ok(None);
    };
    let name = format!("avatars/imported-{}-{n}.png", now_ms());
    crate::files::write_atomic(&paths.stored(&name)?, &bytes)?;
    Ok(Some(name))
}

async fn add_rows(
    staged: &Db,
    live: &Db,
    paths: &AppPaths,
    dir: &Path,
    added: &mut (Vec<i64>, Vec<i64>),
) -> Result<Added> {
    let keep = crate::avatar::AvatarChange::Keep;
    let mut done = Added::default();
    let have = live.characters().await?;
    for (n, c) in staged.characters().await?.into_iter().enumerate() {
        let same = have.iter().any(|h| {
            h.name == c.name && h.personality == c.personality && h.greeting == c.greeting
        });
        if same {
            done.skipped += 1;
            continue;
        }
        let extras = staged.character_extras(c.id).await?;
        // The creator's checks again: the live data must never hold a bad row.
        let saved = crate::library::save_character(
            live,
            paths,
            None,
            &character_fields(&c),
            &keep,
            Some(&extras),
        )
        .await?;
        added.0.push(saved.id);
        if let Some(avatar) = copy_avatar(paths, dir, c.avatar_path.as_deref(), n)? {
            live.set_character_avatar(saved.id, Some(&avatar)).await?;
        }
        let kept = staged.kept(c.id).await?;
        live.set_memory_mode(saved.id, &kept.mode).await?;
        for m in kept.memories {
            if let Some(note) = crate::memory::user_note(&m.content, "fact") {
                live.keep_memory(saved.id, &note).await?;
            }
        }
        done.characters += 1;
    }
    let have = live.personas().await?;
    for (n, p) in staged.personas().await?.into_iter().enumerate() {
        if have
            .iter()
            .any(|h| h.name == p.name && h.description == p.description)
        {
            done.skipped += 1;
            continue;
        }
        let saved =
            crate::library::save_persona(live, paths, None, &persona_fields(&p), &keep).await?;
        added.1.push(saved.id);
        if let Some(avatar) = copy_avatar(paths, dir, p.avatar_path.as_deref(), n + 100_000)? {
            live.set_persona_avatar(saved.id, Some(&avatar)).await?;
        }
        done.personas += 1;
    }
    Ok(done)
}

/// "Add characters only": adds the characters and personas of an opened
/// backup, with their pictures, world info and kept facts, as new ones.
/// Chats and the gallery stay out. One that is already here with the same
/// text is skipped. A failure removes what this call added.
pub async fn add_characters(live: &Db, paths: &AppPaths, id: &str) -> Result<Added> {
    let (dir, _) = staging(paths, id)?;
    let file = dir.join(DATABASE);
    if !file.exists() {
        return Err(Error::Invalid("this backup is no longer open".into()));
    }
    let staged = Db::open(&file).await?;
    let mut added = (Vec::new(), Vec::new());
    let result = add_rows(&staged, live, paths, &dir, &mut added).await;
    staged.close().await;
    if result.is_err() {
        for character in added.0 {
            if let Err(e) = crate::library::delete_character(live, paths, character).await {
                log::warn!("could not remove a half-added character: {e}");
            }
        }
        for persona in added.1 {
            if let Err(e) = crate::library::delete_persona(live, paths, persona).await {
                log::warn!("could not remove a half-added persona: {e}");
            }
        }
    } else {
        discard(paths, id);
    }
    result
}

/// The note that asks the next start to replace the data with a backup:
/// `<import id> <time>`.
const PENDING: &str = "pending-import.txt";

fn digits(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.bytes().all(|b| b.is_ascii_digit())
}

/// "Replace": the next start swaps the data for the opened backup
/// (`apply_pending`). The app must restart now, so nothing holds the files.
pub fn ask_replace(paths: &AppPaths, id: &str) -> Result<()> {
    let (staged, _) = staging(paths, id)?;
    if !staged.join(DATABASE).exists() {
        return Err(Error::Invalid("this backup is no longer open".into()));
    }
    let note = format!("{id} {}", now_ms());
    crate::files::write_atomic(&paths.backups().join(PENDING), note.as_bytes())
}

/// Takes back `ask_replace` when the restart did not happen, so no later
/// start swaps the data without the user knowing.
pub fn cancel_replace(paths: &AppPaths) {
    let note = paths.backups().join(PENDING);
    if let Err(e) = std::fs::remove_file(&note)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::error!("could not remove the import note: {e}");
    }
}

/// The moves of a Replace in order: the current data aside into
/// `replaced-import-<time>`, then the staged backup in. Paths only from
/// fixed names and the two checked numbers.
fn replace_moves(paths: &AppPaths, id: &str, stamp: &str) -> Result<Vec<(PathBuf, PathBuf)>> {
    if !digits(stamp) {
        return Err(Error::Invalid("the import note is damaged".into()));
    }
    let (staged, staged_gallery) = staging(paths, id)?;
    let aside = paths.backups().join(format!("replaced-import-{stamp}"));
    let db = paths.database();
    let journal = |base: &Path, suffix: &str| PathBuf::from(format!("{}{suffix}", base.display()));
    let mut moves = Vec::new();
    // The chats file with its journal files, or none of them.
    for suffix in ["", "-wal", "-shm"] {
        moves.push((journal(&db, suffix), journal(&aside.join(DATABASE), suffix)));
    }
    moves.push((paths.avatars(), aside.join("avatars")));
    moves.push((
        paths.gallery(),
        paths.library().join(format!("gallery-replaced-{stamp}")),
    ));
    for suffix in ["", "-wal", "-shm"] {
        moves.push((
            journal(&staged.join(DATABASE), suffix),
            journal(&db, suffix),
        ));
    }
    moves.push((staged.join("avatars"), paths.avatars()));
    moves.push((staged_gallery, paths.gallery()));
    Ok(moves)
}

/// Moves back what `moves` moved, newest first. Safe to run on a
/// half-done Replace: it moves only what is at the target and not at the source.
fn undo(moves: &[(PathBuf, PathBuf)]) {
    for (from, to) in moves.iter().rev() {
        if to.exists()
            && !from.exists()
            && let Err(e) = std::fs::rename(to, from)
        {
            log::error!("could not move {} back: {e}", to.display());
        }
    }
}

/// Runs a Replace that the last run asked for, before the chats file
/// opens. `Ok(false)`: none asked. On a failure, and after a crash part
/// way, everything moves back and the error says so. The data before the
/// Replace stays in `backups/replaced-import-<time>`.
pub fn apply_pending(paths: &AppPaths) -> Result<bool> {
    let note_file = paths.backups().join(PENDING);
    let Ok(note) = std::fs::read_to_string(&note_file) else {
        return Ok(false);
    };
    let give_up = |e: Error| {
        if let Err(r) = std::fs::remove_file(&note_file) {
            log::error!("could not remove the import note: {r}");
        }
        e
    };
    let mut parts = note.split_whitespace();
    let (Some(id), Some(stamp)) = (parts.next(), parts.next()) else {
        return Err(give_up(Error::Invalid("the import note is damaged".into())));
    };
    let moves = replace_moves(paths, id, stamp).map_err(give_up)?;
    let aside = paths.backups().join(format!("replaced-import-{stamp}"));
    if aside.exists() {
        // An earlier start stopped part way (a crash or a power cut).
        undo(&moves);
        return Err(give_up(Error::Invalid(
            "the backup import stopped part way. Your data is back as it was before".into(),
        )));
    }
    let (staged, _) = staging(paths, id).map_err(give_up)?;
    if !staged.join(DATABASE).exists() {
        return Err(give_up(Error::Invalid(
            "the backup to import is gone".into(),
        )));
    }
    let moved = (|| -> Result<()> {
        std::fs::create_dir_all(&aside)?;
        for (i, (from, to)) in moves.iter().enumerate() {
            if !from.exists() {
                continue;
            }
            // A rename on Windows can replace what is at the target: never let it.
            let moved = if to.exists() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} is in the way", to.display()),
                ))
            } else {
                std::fs::rename(from, to)
            };
            if let Err(e) = moved {
                undo(&moves[..i]);
                return Err(e.into());
            }
        }
        // While the note is there, the next start undoes the Replace.
        if let Err(e) = std::fs::remove_file(&note_file) {
            undo(&moves);
            return Err(e.into());
        }
        Ok(())
    })();
    match moved {
        Ok(()) => {
            discard(paths, id);
            log::info!(
                "the data is replaced by the backup; the old data is in {}",
                aside.display()
            );
            Ok(true)
        }
        Err(e) => Err(give_up(Error::Invalid(format!(
            "the backup could not replace your data ({e}). Your data is as it was before"
        )))),
    }
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

    fn png() -> Vec<u8> {
        let picture = image::DynamicImage::new_rgba8(64, 64);
        let mut out = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in entries {
            zip.start_file(*name, stored).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A home with one of each: character with a picture, persona, chat and gallery picture.
    async fn home(dir: &Path) -> (AppPaths, Db) {
        let paths = AppPaths::at(dir.join("home"));
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();
        let fields = CharacterFields {
            name: "Ren".into(),
            personality: "Calm.".into(),
            greeting: "Sit.".into(),
            default_tone: "casual".into(),
            default_reply_style: "texting".into(),
            age: 30,
            ..Default::default()
        };
        let id = db.insert_character(&fields, None, "{}").await.unwrap();
        std::fs::write(paths.avatars().join("ren.png"), png()).unwrap();
        db.set_character_avatar(id, Some("avatars/ren.png"))
            .await
            .unwrap();
        let persona = PersonaFields {
            name: "Kai".into(),
            age: Some(25),
            ..Default::default()
        };
        db.insert_persona(&persona, None).await.unwrap();
        let character = db.character(id).await.unwrap();
        let chat = db.create_chat(&character, None, "", true).await.unwrap();
        std::fs::create_dir_all(paths.gallery().join(chat.to_string())).unwrap();
        std::fs::write(paths.gallery().join(format!("{chat}/p.png")), png()).unwrap();
        db.insert_image(&crate::db::NewImage {
            chat_id: chat,
            message_id: None,
            file_path: format!("{chat}/p.png"),
            prompt: String::new(),
            negative_prompt: String::new(),
            seed: 1,
            image_model: "m".into(),
            ref_mode: "tags".into(),
            width: 64,
            height: 64,
            adult: false,
        })
        .await
        .unwrap();
        (paths, db)
    }

    #[tokio::test]
    async fn an_exported_backup_opens_with_its_counts_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, db) = home(dir.path()).await;
        let zip = dir.path().join("b.zip");
        export(&db, &paths, &zip, "1").await.unwrap();

        let info = open(&paths, &zip).await.unwrap();
        let counts = [info.characters, info.personas, info.chats, info.pictures];
        assert_eq!(counts, [1, 1, 1, 1]);
        let (staged, gallery) = staging(&paths, &info.id).unwrap();
        let avatar = std::fs::read(staged.join("avatars").join("ren.png")).unwrap();
        assert!(image::load_from_memory(&avatar).is_ok(), "a clean PNG copy");
        assert_eq!(
            std::fs::read(gallery.join("1").join("p.png")).unwrap(),
            png()
        );
        // The current data did not change, and the staged files go at the next start.
        assert_eq!(db.counts().await.unwrap(), [1, 1, 1, 1]);
        clear_leftovers(&paths);
        assert!(!staged.exists() && !gallery.exists());
    }

    #[tokio::test]
    async fn add_characters_only_adds_new_ones_with_their_parts_and_no_chats() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, db) = home(dir.path()).await;
        let ren = db.characters().await.unwrap()[0].id;
        db.keep_memory(ren, "Kai has a sister.").await.unwrap();
        db.set_memory_mode(ren, "always").await.unwrap();
        db.add_world_entries(
            ren,
            &[crate::db::WorldEntry {
                keywords: "office".into(),
                content: "Rain on the window.".into(),
                is_enabled: true,
            }],
        )
        .await
        .unwrap();
        let zip = dir.path().join("b.zip");
        export(&db, &paths, &zip, "1").await.unwrap();

        // Into this same home: Ren and Kai are here already.
        let info = open(&paths, &zip).await.unwrap();
        let same = add_characters(&db, &paths, &info.id).await.unwrap();
        assert_eq!((same.characters, same.personas, same.skipped), (0, 0, 2));

        // Into another home: both come in with new ids, no chat.
        let other = tempfile::tempdir().unwrap();
        let paths2 = AppPaths::at(other.path().join("home"));
        paths2.ensure().unwrap();
        let live = Db::open(&paths2.database()).await.unwrap();
        let info = open(&paths2, &zip).await.unwrap();
        let added = add_characters(&live, &paths2, &info.id).await.unwrap();
        assert_eq!((added.characters, added.personas), (1, 1));
        assert_eq!(live.counts().await.unwrap(), [1, 1, 0, 0]);
        let c = &live.characters().await.unwrap()[0];
        let avatar = c.avatar_path.clone().unwrap();
        assert!(paths2.stored(&avatar).unwrap().exists());
        let kept = live.kept(c.id).await.unwrap();
        assert_eq!(kept.mode, "always");
        assert_eq!(kept.memories[0].content, "Kai has a sister.");
        assert_eq!(live.character_extras(c.id).await.unwrap().world.len(), 1);
        // The staged files are gone after a good add.
        let (staged, gallery) = staging(&paths2, &info.id).unwrap();
        assert!(!staged.exists() && !gallery.exists());
        assert!(add_characters(&live, &paths2, &info.id).await.is_err());
    }

    /// A home with a backup of another home opened in it.
    async fn home_with_backup(dir: &Path) -> (AppPaths, BackupInfo) {
        let (other, db) = home(&dir.join("other")).await;
        db.insert_persona(
            &PersonaFields {
                name: "Second".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        let zip = dir.join("b.zip");
        export(&db, &other, &zip, "1").await.unwrap();
        db.close().await;
        let paths = AppPaths::at(dir.join("home"));
        paths.ensure().unwrap();
        Db::open(&paths.database()).await.unwrap().close().await;
        std::fs::write(paths.avatars().join("mine.png"), png()).unwrap();
        let info = open(&paths, &zip).await.unwrap();
        (paths, info)
    }

    #[tokio::test]
    async fn replace_swaps_the_data_and_keeps_the_old_aside() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, info) = home_with_backup(dir.path()).await;
        assert!(!apply_pending(&paths).unwrap(), "none asked");
        ask_replace(&paths, &info.id).unwrap();

        assert!(apply_pending(&paths).unwrap());
        let db = Db::open(&paths.database()).await.unwrap();
        assert_eq!(db.counts().await.unwrap(), [1, 2, 1, 1]);
        assert!(paths.avatars().join("ren.png").exists());
        assert!(!paths.avatars().join("mine.png").exists());
        assert!(paths.gallery().join("1").join("p.png").exists());
        let aside = std::fs::read_dir(paths.backups())
            .unwrap()
            .flatten()
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("replaced-import-")
            })
            .unwrap()
            .path();
        assert!(aside.join(DATABASE).exists() && aside.join("avatars").join("mine.png").exists());
        assert!(!paths.backups().join(PENDING).exists());
        assert!(!apply_pending(&paths).unwrap(), "runs once");
    }

    #[tokio::test]
    async fn a_failed_or_half_done_replace_puts_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, info) = home_with_backup(dir.path()).await;
        let before = std::fs::read(paths.database()).unwrap();
        ask_replace(&paths, &info.id).unwrap();
        let note = std::fs::read_to_string(paths.backups().join(PENDING)).unwrap();
        let stamp = note.split_whitespace().nth(1).unwrap().to_owned();
        // A file where the old gallery must go: that move fails, after the
        // chats file and the avatars moved aside.
        std::fs::create_dir_all(paths.gallery()).unwrap();
        std::fs::write(
            paths.library().join(format!("gallery-replaced-{stamp}")),
            b"x",
        )
        .unwrap();
        let e = apply_pending(&paths).unwrap_err().to_string();
        assert!(e.contains("could not replace"), "{e}");
        assert_eq!(std::fs::read(paths.database()).unwrap(), before);
        assert!(paths.avatars().join("mine.png").exists());
        assert!(!paths.backups().join(PENDING).exists());

        // A crash part way: the chats file is aside and the note is still there.
        std::fs::remove_file(paths.library().join(format!("gallery-replaced-{stamp}"))).unwrap();
        let info = open(&paths, &dir.path().join("b.zip")).await.unwrap();
        ask_replace(&paths, &info.id).unwrap();
        let note = std::fs::read_to_string(paths.backups().join(PENDING)).unwrap();
        let stamp = note.split_whitespace().nth(1).unwrap();
        let aside = paths.backups().join(format!("replaced-import-{stamp}"));
        std::fs::create_dir_all(&aside).unwrap();
        std::fs::rename(paths.database(), aside.join(DATABASE)).unwrap();
        let e = apply_pending(&paths).unwrap_err().to_string();
        assert!(e.contains("part way"), "{e}");
        assert_eq!(std::fs::read(paths.database()).unwrap(), before);
    }

    /// The checks must pass on what the app itself makes: the starters.
    #[tokio::test]
    async fn a_backup_with_the_starters_opens() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();
        crate::starters::ensure(&db, &paths).await.unwrap();
        // A backup never brings the adult setting: the user confirms 18+ here.
        crate::content::set_adult(&db, true).await.unwrap();
        let zip = dir.path().join("b.zip");
        export(&db, &paths, &zip, "1").await.unwrap();
        let info = open(&paths, &zip).await.unwrap();
        assert!(info.characters > 0 && info.personas > 0);
        let (staged, _) = staging(&paths, &info.id).unwrap();
        let copy = Db::open(&staged.join(DATABASE)).await.unwrap();
        assert!(!crate::content::adult_on(&copy).await.unwrap());
        copy.close().await;
        assert!(
            crate::content::adult_on(&db).await.unwrap(),
            "this install keeps it"
        );
    }

    /// The same on real data: a copy of an app folder that a person used.
    /// `UNSTRUNG_TEST_BACKUP_HOME` names it; only its chats file, avatars
    /// and gallery are read.
    #[tokio::test]
    #[ignore = "needs UNSTRUNG_TEST_BACKUP_HOME"]
    async fn a_backup_of_real_data_opens() {
        let from = AppPaths::at(PathBuf::from(
            std::env::var("UNSTRUNG_TEST_BACKUP_HOME").unwrap(),
        ));
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        // Plain file copies: the real folder is only read. Its app must be closed.
        for suffix in ["", "-wal"] {
            let file = PathBuf::from(format!("{}{suffix}", from.database().display()));
            if file.exists() {
                let to = format!("{}{suffix}", paths.database().display());
                std::fs::copy(file, to).unwrap();
            }
        }
        let mut files = Vec::new();
        pictures(&from.avatars(), "avatars", &mut files).unwrap();
        pictures(&from.gallery(), "gallery", &mut files).unwrap();
        for (f, name) in files {
            // The temp home keeps its library in the app folder.
            let to = join_safe(paths.root(), &name).unwrap();
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(f, to).unwrap();
        }
        let db = Db::open(&paths.database()).await.unwrap();
        let zip = dir.path().join("b.zip");
        export(&db, &paths, &zip, "1").await.unwrap();
        let info = open(&paths, &zip).await.unwrap();
        println!("{info:?}");
    }

    #[tokio::test]
    async fn names_that_leave_the_folder_refuse_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let picture = png();
        for bad in [
            "gallery/../../x.png",
            "gallery/..\\..\\x.png",
            "gallery/C:/x.png",
            "gallery/1/CON.png",
            "gallery/1/a:b.png",
            "avatars/../x.png",
            "avatars/sub/x.png",
            "avatars/x.jpg",
        ] {
            let zip = dir.path().join("bad.zip");
            zip_of(
                &zip,
                &[(MANIFEST, b"{}"), (DATABASE, b"x"), (bad, &picture)],
            );
            let e = open(&paths, &zip).await.unwrap_err().to_string();
            assert!(
                e.contains("file name") || e.contains("unexpected"),
                "{bad}: {e}"
            );
        }
        // An unknown name outside the two folders is skipped, never written.
        let zip = dir.path().join("skip.zip");
        zip_of(&zip, &[("../evil.png", &picture)]);
        assert!(open(&paths, &zip).await.is_err(), "no chats file in it");
        assert!(!dir.path().join("evil.png").exists());
        assert!(!dir.path().join("x.png").exists());
        let staged = std::fs::read_dir(paths.backups()).unwrap().count();
        assert_eq!(staged, 0, "a refused backup leaves nothing");
    }

    #[tokio::test]
    async fn two_names_that_differ_only_in_case_refuse_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let zip = dir.path().join("b.zip");
        let picture = png();
        zip_of(
            &zip,
            &[
                (MANIFEST, b"{}"),
                (DATABASE, b"x"),
                ("gallery/1/A.png", &picture),
                ("gallery/1/a.png", &picture),
            ],
        );
        let e = open(&paths, &zip).await.unwrap_err().to_string();
        assert!(e.contains("twice"), "{e}");
    }

    #[tokio::test]
    async fn a_size_in_the_header_that_lies_refuses_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let zip = dir.path().join("b.zip");
        let big = vec![7u8; 2 << 20];
        zip_of(
            &zip,
            &[
                ("gallery/1/a.png", &big),
                (MANIFEST, b"{}"),
                (DATABASE, b"x"),
            ],
        );
        // Says 10 bytes, holds 2 MB: patch the size in both headers of the first file.
        let mut bytes = std::fs::read(&zip).unwrap();
        for (signature, at) in [(b"PK\x03\x04", 22), (b"PK\x01\x02", 24)] {
            let start = bytes.windows(4).position(|w| w == signature).unwrap();
            bytes[start + at..start + at + 4].copy_from_slice(&10u32.to_le_bytes());
        }
        std::fs::write(&zip, bytes).unwrap();
        let e = open(&paths, &zip).await.unwrap_err().to_string();
        assert!(e.contains("damaged"), "{e}");
        let staged = std::fs::read_dir(paths.backups()).unwrap().count();
        assert_eq!(staged, 0);
    }

    #[tokio::test]
    async fn a_newer_chats_file_or_a_bad_row_refuses_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, db) = home(dir.path()).await;
        sqlx::query("UPDATE characters SET default_tone = 'nonsense'")
            .execute(db.pool())
            .await
            .unwrap();
        let zip = dir.path().join("bad-row.zip");
        export(&db, &paths, &zip, "1").await.unwrap();
        let e = open(&paths, &zip).await.unwrap_err().to_string();
        assert!(e.contains("Ren"), "names the bad item: {e}");

        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (999, 'later', 1, x'00', 0)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        let zip = dir.path().join("newer.zip");
        export(&db, &paths, &zip, "1").await.unwrap();
        let e = open(&paths, &zip).await.unwrap_err().to_string();
        assert!(e.contains("newer"), "{e}");
    }
}
