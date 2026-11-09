use super::*;
use std::collections::BTreeSet;

#[tokio::test]
async fn an_export_holds_the_chats_file_and_the_pictures_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(dir.path().join("home"));
    paths.ensure().unwrap();
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
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
#[test]
fn the_check_after_writing_finds_a_damaged_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("b.zip");
    let payload = b"a picture payload that the check must read to its end";
    let mut zip = ZipWriter::new(File::create(&file).unwrap());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("gallery/1/a.png", stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap();
    check_zip(&file, 1, None).unwrap();
    assert!(check_zip(&file, 2, None).is_err(), "a missing file");
    let mut bytes = std::fs::read(&file).unwrap();
    let at = bytes
        .windows(payload.len())
        .position(|w| w == payload)
        .unwrap();
    bytes[at + 5] ^= 0xff;
    std::fs::write(&file, bytes).unwrap();
    assert!(check_zip(&file, 1, None).is_err(), "a changed byte");
}

async fn home(dir: &Path) -> (AppPaths, Db) {
    let paths = AppPaths::at(dir.join("home"));
    paths.ensure().unwrap();
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
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
    // The zip holds a plain chats file; the opened copy is encrypted.
    assert!(!crate::vault::is_plain_database(&staged.join(DATABASE)));
    let key = paths.key().unwrap();
    let avatar = std::fs::read(staged.join("avatars").join("ren.png")).unwrap();
    assert!(crate::vault::is_sealed(&avatar), "encrypted on disk");
    let avatar = key.open(&avatar).unwrap();
    assert!(image::load_from_memory(&avatar).is_ok(), "a clean PNG copy");
    assert_eq!(key.read(&gallery.join("1").join("p.png")).unwrap(), png());
    // The current data did not change, and the staged files go at the next start.
    assert_eq!(db.counts().await.unwrap(), [1, 1, 1, 1]);
    clear_leftovers(&paths);
    assert!(!staged.exists() && !gallery.exists());
}

#[tokio::test]
async fn a_backup_with_a_password_hides_its_files_and_opens_only_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = home(dir.path()).await;
    assert!(set_password(&db, Some("short")).await.is_err(), "too short");
    set_password(&db, Some("correct horse")).await.unwrap();
    assert!(status(&db, now_ms()).await.unwrap().password);
    let zip = dir.path().join("b.zip");
    export(&db, &paths, &zip, "1").await.unwrap();

    // Nothing readable in it: not the chats file, not the picture.
    let raw = std::fs::read(&zip).unwrap();
    assert!(!raw.windows(15).any(|w| w == b"SQLite format 3"));
    let picture = png();
    assert!(!raw.windows(picture.len()).any(|w| w == picture.as_slice()));
    let mut archive = ZipArchive::new(File::open(&zip).unwrap()).unwrap();
    let mut notes = String::new();
    archive
        .by_name(MANIFEST)
        .unwrap()
        .read_to_string(&mut notes)
        .unwrap();
    let manifest: Manifest = serde_json::from_str(&notes).unwrap();
    assert_eq!(manifest.format, FORMAT_SEALED);

    assert!(matches!(
        open(&paths, &zip).await,
        Err(Error::PasswordNeeded)
    ));
    assert!(matches!(
        open_with(&paths, &zip, Some("wrong horse".into())).await,
        Err(Error::WrongPassword)
    ));
    let info = open_with(&paths, &zip, Some("correct horse".into()))
        .await
        .unwrap();
    assert_eq!(
        [info.characters, info.personas, info.chats, info.pictures],
        [1, 1, 1, 1]
    );
    let (_, gallery) = staging(&paths, &info.id).unwrap();
    let key = paths.key().unwrap();
    assert_eq!(key.read(&gallery.join("1").join("p.png")).unwrap(), png());

    // Without the password, exports are plain again.
    set_password(&db, None).await.unwrap();
    let plain = dir.path().join("plain.zip");
    export(&db, &paths, &plain, "1").await.unwrap();
    assert!(open(&paths, &plain).await.is_ok());
}

#[tokio::test]
async fn the_reminder_comes_30_days_after_the_last_backup_or_later() {
    let dir = tempfile::tempdir().unwrap();
    let (_paths, db) = home(dir.path()).await;
    let day = 24 * 60 * 60 * 1000;
    let start = now_ms();
    let first = status(&db, start).await.unwrap();
    assert!(
        first.last_ms.is_none() && !first.due,
        "the clock starts at the first check"
    );
    assert!(status(&db, start + 30 * day).await.unwrap().due);
    snooze(&db, start + 30 * day).await.unwrap();
    assert!(!status(&db, start + 59 * day).await.unwrap().due);
    assert!(status(&db, start + 60 * day).await.unwrap().due);

    // A backup restarts the wait: a fresh home, so no "Later" hides it.
    let dir = tempfile::tempdir().unwrap();
    let (paths, db) = home(dir.path()).await;
    status(&db, start - 60 * day).await.unwrap();
    assert!(status(&db, start).await.unwrap().due);
    export(&db, &paths, &dir.path().join("b.zip"), "1")
        .await
        .unwrap();
    let after = status(&db, now_ms()).await.unwrap();
    assert!(after.last_ms.is_some_and(|t| t >= start) && !after.due);
    assert!(!status(&db, now_ms() + 29 * day).await.unwrap().due);
    assert!(status(&db, now_ms() + 31 * day).await.unwrap().due);
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
    let live = Db::open(&paths2.database(), &paths2.key().unwrap())
        .await
        .unwrap();
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
    Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap()
        .close()
        .await;
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
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
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
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    crate::starters::ensure(&db, &paths).await.unwrap();
    // A backup never brings the adult setting: the user confirms 18+ here.
    crate::content::set_adult(&db, true).await.unwrap();
    let zip = dir.path().join("b.zip");
    export(&db, &paths, &zip, "1").await.unwrap();
    let info = open(&paths, &zip).await.unwrap();
    assert!(info.characters > 0 && info.personas > 0);
    let (staged, _) = staging(&paths, &info.id).unwrap();
    let copy = Db::open(&staged.join(DATABASE), &paths.key().unwrap())
        .await
        .unwrap();
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
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
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
    // Good notes, so the import reaches the picture.
    let notes = br#"{"format":1,"schema":0,"app_version":"1","created_ms":0}"#;
    zip_of(
        &zip,
        &[
            ("gallery/1/a.png", &big),
            (MANIFEST, notes),
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
