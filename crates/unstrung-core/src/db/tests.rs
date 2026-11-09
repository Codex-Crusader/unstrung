use super::*;

fn ren() -> CharacterFields {
    CharacterFields {
        name: "Ren".into(),
        personality: "Tired.".into(),
        greeting: "Sit. Talk.".into(),
        default_tone: "dark".into(),
        default_reply_style: "conversational".into(),
        default_reply_custom: String::new(),
        art_style: String::new(),
        age: 34,
        ..Default::default()
    }
}

async fn user(db: &Db, chat: i64, parent: Option<i64>, text: &str) -> Message {
    db.append_message(&NewMessage {
        chat_id: chat,
        parent_id: parent,
        role: "user",
        content: text,
        ..Default::default()
    })
    .await
    .unwrap()
}

fn texts(path: &[Message]) -> Vec<&str> {
    path.iter().map(|m| m.content.as_str()).collect()
}

#[tokio::test]
async fn message_tree_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert!(db.quick_check().await.unwrap());
    let id = db
        .insert_character(&ren(), Some("test"), "{}")
        .await
        .unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let hello = user(&db, chat, Some(greeting), "Hello").await;
    // A sibling branch that is not on the path to the head.
    user(&db, chat, Some(greeting), "Other branch").await;
    user(&db, chat, Some(hello.id), "Talk.").await;
    db.close().await;

    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    let path = db.path_to_head(chat).await.unwrap();
    assert_eq!(texts(&path), ["Sit. Talk.", "Hello", "Talk."]);
    assert_eq!(db.latest_chat(id).await.unwrap(), Some(chat));
    assert_eq!(
        db.meta("schema_version").await.unwrap().as_deref(),
        Some("8")
    );
    let summaries = db.chat_summaries(id).await.unwrap();
    assert_eq!(summaries[0].message_count, 4);
    assert_eq!(summaries[0].preview, "Hello");
}

#[tokio::test]
async fn delete_restore_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let a = user(&db, chat, Some(greeting), "a").await;
    let b = user(&db, chat, Some(a.id), "b").await;

    db.delete_subtree(chat, a.id, 77).await.unwrap();
    db.set_chat_head(chat, Some(greeting)).await.unwrap();
    assert_eq!(texts(&db.path_to_head(chat).await.unwrap()), ["Sit. Talk."]);
    assert!(
        db.message(chat, b.id).await.is_err(),
        "deleted messages are hidden"
    );
    assert_eq!(db.tree_links(chat).await.unwrap().len(), 1);

    db.restore_subtree(chat, a.id, 77).await.unwrap();
    db.set_chat_head(chat, Some(b.id)).await.unwrap();
    let path = db.path_to_head(chat).await.unwrap();
    assert_eq!(texts(&path), ["Sit. Talk.", "a", "b"]);

    let copy = db
        .create_chat(&character, None, "Branch", false)
        .await
        .unwrap();
    db.copy_path(copy, &path[..2]).await.unwrap();
    assert_eq!(
        texts(&db.path_to_head(copy).await.unwrap()),
        ["Sit. Talk.", "a"]
    );
    assert_eq!(
        texts(&db.path_to_head(chat).await.unwrap()),
        ["Sit. Talk.", "a", "b"]
    );
}

#[tokio::test]
async fn personas_and_world_info() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let kai = db
        .insert_persona(
            &PersonaFields {
                name: "Kai".into(),
                age: Some(25),
                ..Default::default()
            },
            Some("kai"),
        )
        .await
        .unwrap();
    db.set_default_persona(Some(kai)).await.unwrap();
    assert_eq!(db.default_persona().await.unwrap(), Some(kai));
    assert!(db.persona(kai).await.unwrap().is_default);

    let id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let entry = WorldEntry {
        keywords: "east door".into(),
        content: "Sealed.".into(),
        is_enabled: true,
    };
    db.add_world_entries(id, &[entry.clone(), entry])
        .await
        .unwrap();
    assert_eq!(db.world_entry_count(id).await.unwrap(), 2);
    db.delete_character(id).await.unwrap();
    assert_eq!(db.world_entry_count(id).await.unwrap(), 0);
}

#[tokio::test]
async fn first_run_stays_pending_until_done() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert!(db.first_run_pending().await.unwrap());
    db.close().await;
    // Closing the app during the welcome flow keeps it pending.
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert!(db.first_run_pending().await.unwrap());
    db.set_meta(FIRST_RUN_KEY, "done").await.unwrap();
    db.close().await;
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert!(!db.first_run_pending().await.unwrap());
}

#[tokio::test]
async fn a_database_from_before_the_welcome_flow_skips_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    // An older build had no first-run flag.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(FIRST_RUN_KEY)
        .execute(&db.pool)
        .await
        .unwrap();
    db.close().await;
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert_eq!(
        db.meta(FIRST_RUN_KEY).await.unwrap().as_deref(),
        Some("existing")
    );
    assert!(!db.first_run_pending().await.unwrap());
}

#[tokio::test]
async fn recent_chats_are_talked_in_newest_first_and_not_adult() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let ren_id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let detective = db.character(ren_id).await.unwrap();
    let adult_fields = CharacterFields {
        name: "Vex".into(),
        is_adult: true,
        ..ren()
    };
    let vex = db
        .character(
            db.insert_character(&adult_fields, None, "{}")
                .await
                .unwrap(),
        )
        .await
        .unwrap();

    // Only the greeting: not recent.
    db.create_chat(&detective, None, "", true).await.unwrap();
    let older = db.create_chat(&detective, None, "", true).await.unwrap();
    let greeting = db.chat(older).await.unwrap().head_message_id;
    user(&db, older, greeting, "First chat").await;
    let newer = db.create_chat(&detective, None, "", true).await.unwrap();
    let greeting = db.chat(newer).await.unwrap().head_message_id;
    let hello = user(&db, newer, greeting, "Hello").await;
    user(&db, newer, greeting, "Other branch").await;
    // The preview is the head: the last message on the current branch.
    user(&db, newer, Some(hello.id), "Where were we?").await;
    let gone = db.create_chat(&detective, None, "", true).await.unwrap();
    user(&db, gone, None, "Deleted").await;
    db.delete_chat(gone).await.unwrap();
    let adult = db.create_chat(&vex, None, "", true).await.unwrap();
    user(&db, adult, None, "Hidden").await;

    let recent = db.recent_chats(8, false).await.unwrap();
    let ids: Vec<i64> = recent.iter().map(|r| r.chat_id).collect();
    assert_eq!(ids, [newer, older]);
    assert_eq!(recent[0].preview, "Where were we?");
    assert_eq!(recent[0].character_name, "Ren");
    assert_eq!(db.recent_chats(1, false).await.unwrap().len(), 1);
}

/// A database made by an older app: only the first `count` migrations.
pub(crate) async fn old_database(dir: &Path, count: usize) -> std::path::PathBuf {
    let sql = dir.join("old-migrations");
    std::fs::create_dir_all(&sql).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    for f in files.iter().take(count) {
        std::fs::copy(f, sql.join(f.file_name().unwrap())).unwrap();
    }
    let file = dir.join("app.db");
    let options = SqliteConnectOptions::new()
        .filename(&file)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(sql.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    pool.close().await;
    file
}

fn copies(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir.join("backups")).map_or(Vec::new(), |entries| {
        entries
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect()
    })
}

#[tokio::test]
async fn an_update_copies_the_database_first() {
    let dir = tempfile::tempdir().unwrap();
    let file = old_database(dir.path(), 3).await;
    Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap()
        .close()
        .await;
    let made = copies(dir.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].starts_with("before-schema-3-"), "{made:?}");
    let copy = dir.path().join("backups").join(&made[0]);
    assert_eq!(
        Db::integrity_check_file(&copy, &crate::vault::DataKey::test())
            .await
            .unwrap(),
        Some(Vec::new())
    );
    // Opening again changes nothing, so it makes no second copy.
    Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap()
        .close()
        .await;
    assert_eq!(copies(dir.path()).len(), 1);
}

#[tokio::test]
async fn a_new_database_makes_no_copy() {
    let dir = tempfile::tempdir().unwrap();
    Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap()
        .close()
        .await;
    assert!(copies(dir.path()).is_empty());
}

#[tokio::test]
async fn only_the_newest_copies_stay() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    let mut made = Vec::new();
    for _ in 0..KEEP_BACKUPS + 2 {
        made.push(backup(&db.pool, &file, 4).await.unwrap());
    }
    db.close().await;
    let mut left = copies(dir.path());
    left.sort();
    let mut newest: Vec<String> = made[2..]
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    newest.sort();
    assert_eq!(left, newest);
}

#[tokio::test]
async fn pictures_are_saved_listed_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id;
    let new = |file: &str| crate::db::NewImage {
        chat_id: chat,
        message_id: greeting,
        file_path: format!("{chat}/{file}"),
        prompt: "a tavern".into(),
        negative_prompt: String::new(),
        seed: 42,
        image_model: "z-image-turbo".into(),
        ref_mode: "tags".into(),
        width: 1024,
        height: 1024,
        adult: false,
    };
    let first = db.insert_image(&new("a.png")).await.unwrap();
    db.insert_image(&new("b.png")).await.unwrap();
    assert_eq!(db.chat_images(chat).await.unwrap().len(), 2);
    let gallery = db.gallery(false).await.unwrap();
    assert_eq!(gallery.len(), 2);
    assert_eq!(gallery[0].character_name, "Ren");
    assert_eq!(gallery[1].image.id, first, "newest first");
    assert_eq!(db.image(first).await.unwrap().seed, 42);

    assert_eq!(
        db.delete_image(first).await.unwrap(),
        format!("{chat}/a.png")
    );
    assert!(db.image(first).await.is_err());

    // A deleted chat leaves the gallery, but its files are still known.
    db.delete_chat(chat).await.unwrap();
    assert!(db.gallery(false).await.unwrap().is_empty());
    assert_eq!(
        db.character_image_files(id).await.unwrap(),
        [format!("{chat}/b.png")]
    );
    // Deleting the character removes the rows.
    db.delete_character(id).await.unwrap();
    assert!(db.chat_images(chat).await.unwrap().is_empty());
}

#[tokio::test]
async fn one_copy_a_day_and_the_last_seven_stay() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    assert!(db.snapshot_on_day(&file, 1).await.unwrap().is_some());
    // The same day: no second copy.
    assert!(db.snapshot_on_day(&file, 1).await.unwrap().is_none());
    for day in 2..=9 {
        db.snapshot_on_day(&file, day).await.unwrap();
    }
    let list = snapshots::list(&dir.path().join("backups"));
    assert_eq!(list.len(), 7, "{list:?}");
    assert!(list.iter().all(|c| c.kind == "daily"));
    assert!(
        list[0].name.starts_with("daily-9-"),
        "newest first: {list:?}"
    );
    assert!(!list.iter().any(|c| c.name.starts_with("daily-2-")));
    db.close().await;
}

#[tokio::test]
async fn a_copy_comes_back_and_the_replaced_file_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    db.set_meta("story", "old").await.unwrap();
    db.snapshot_on_day(&paths.database(), 1).await.unwrap();
    db.set_meta("story", "new").await.unwrap();
    db.close().await;
    let name = snapshots::list(&paths.backups())[0].name.clone();

    // Only a name from the list.
    for bad in ["../app.db", "daily-1-.db", "app.db"] {
        assert!(snapshots::restore(&paths, bad).await.is_err(), "{bad}");
    }
    snapshots::restore(&paths, &name).await.unwrap();
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    assert_eq!(db.meta("story").await.unwrap().as_deref(), Some("old"));
    db.close().await;
    // The replaced file waits in backups, and is not offered as a copy.
    let aside: Vec<_> = std::fs::read_dir(paths.backups())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("replaced-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert!(aside[0].path().join("app.db").is_file());
    assert_eq!(snapshots::list(&paths.backups()).len(), 1);

    // A damaged copy is refused, and the chats file stays.
    std::fs::write(paths.backups().join("daily-5-123.db"), b"not a database").unwrap();
    let err = snapshots::restore(&paths, "daily-5-123.db")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Damaged(_)), "{err:?}");
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    assert_eq!(db.meta("story").await.unwrap().as_deref(), Some("old"));
    db.close().await;
}

/// A database that opens but fails the quick check: one table page is
/// overwritten.
async fn damaged_database(dir: &Path) -> PathBuf {
    let file = dir.join("app.db");
    let db = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap();
    for i in 0..2000 {
        db.set_meta(&format!("key {i}"), &"x".repeat(40))
            .await
            .unwrap();
    }
    db.close().await;
    let mut bytes = std::fs::read(&file).unwrap();
    let page = 4096;
    let at = bytes.len() / 2 / page * page;
    for b in &mut bytes[at + 8..at + page] {
        *b = 0x5a;
    }
    std::fs::write(&file, bytes).unwrap();
    file
}

#[tokio::test]
async fn a_damaged_file_goes_to_safe_start_only_with_a_copy() {
    // No copy: the chats stay usable.
    let dir = tempfile::tempdir().unwrap();
    let file = damaged_database(dir.path()).await;
    Db::open(&file, &crate::vault::DataKey::test())
        .await
        .unwrap()
        .close()
        .await;
    // With a copy: safe start offers it.
    std::fs::create_dir_all(dir.path().join("backups")).unwrap();
    std::fs::write(dir.path().join("backups").join("daily-1-5.db"), b"copy").unwrap();
    let err = Db::open(&file, &crate::vault::DataKey::test())
        .await
        .err()
        .expect("a damaged file must not open");
    assert!(matches!(err, Error::Damaged(_)), "{err:?}");
}

#[tokio::test]
async fn kept_memories_are_one_per_text_and_go_with_the_character() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let kept = db.kept(id).await.unwrap();
    assert_eq!(kept.mode, "ask", "the default asks at a new chat");
    assert!(kept.memories.is_empty());

    db.keep_memory(id, " Kai owes Ren 200 credits. ")
        .await
        .unwrap();
    db.keep_memory(id, "Kai owes Ren 200 credits.")
        .await
        .unwrap();
    assert!(db.keep_memory(id, "  ").await.is_err());
    let kept = db.kept(id).await.unwrap();
    assert_eq!(kept.memories.len(), 1, "the same text stays one memory");
    assert_eq!(kept.memories[0].content, "Kai owes Ren 200 credits.");

    db.set_memory_mode(id, "always").await.unwrap();
    assert!(db.set_memory_mode(id, "sometimes").await.is_err());
    assert_eq!(db.kept(id).await.unwrap().mode, "always");

    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    assert!(!db.chat(chat).await.unwrap().use_character_memory);
    db.set_chat_use_memory(chat, true).await.unwrap();
    assert!(db.chat(chat).await.unwrap().use_character_memory);

    let other = db.insert_character(&ren(), None, "{}").await.unwrap();
    let memory = db.kept(id).await.unwrap().memories[0].id;
    assert!(
        db.forget_character_memory(other, memory).await.is_err(),
        "only the character's own memory"
    );
    db.delete_character(id).await.unwrap();
    assert!(db.kept(id).await.is_err());
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM character_memories")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(left, 0, "the memories go with the character");
}

/// Opens `file` without a key, as another program would.
async fn readable_without_key(file: &Path) -> bool {
    let Ok(pool) = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(file).read_only(true))
        .await
    else {
        return false;
    };
    let read = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master")
        .fetch_one(&pool)
        .await
        .is_ok();
    pool.close().await;
    read
}

#[tokio::test]
async fn the_chats_file_and_its_copies_are_encrypted_but_a_backup_copy_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let key = crate::vault::DataKey::test();
    // A plain file from before encryption, at an older schema.
    let file = old_database(dir.path(), 3).await;
    assert!(crate::vault::is_plain_database(&file));
    let db = Db::open(&file, &key).await.unwrap();
    db.set_meta("secret", "a line of a chat").await.unwrap();
    let daily = db.snapshot_on_day(&file, 9).await.unwrap().unwrap();
    let plain = dir.path().join("plain.db");
    db.plain_copy_to(&plain).await.unwrap();
    db.close().await;

    let raw = std::fs::read(&file).unwrap();
    assert!(!raw.windows(16).any(|w| w == b"a line of a chat"));
    assert!(!crate::vault::is_plain_database(&file));
    assert!(!readable_without_key(&file).await);
    for copy in std::fs::read_dir(dir.path().join("backups")).unwrap() {
        let copy = copy.unwrap().path();
        assert!(!readable_without_key(&copy).await, "{}", copy.display());
    }
    assert!(!readable_without_key(&daily).await);
    assert_eq!(
        Db::integrity_check_file(&daily, &key).await.unwrap(),
        Some(Vec::new())
    );
    assert!(
        readable_without_key(&plain).await,
        "a backup zip opens anywhere"
    );
    let other = crate::vault::DataKey::random().unwrap();
    assert!(
        Db::open(&file, &other).await.is_err(),
        "another key does not open it"
    );
    let old = dir.path().join("backups").join("daily-1-1.db");
    std::fs::copy(&plain, &old).unwrap();
    assert_eq!(
        encrypt_old_copies(&dir.path().join("backups"), &key).await,
        1
    );
    assert!(!readable_without_key(&old).await);
    let db = Db::open(&file, &key).await.unwrap();
    assert_eq!(
        db.meta("secret").await.unwrap().as_deref(),
        Some("a line of a chat")
    );
}

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// Run once to make the files that `files_from_an_earlier_build_still_open`
/// reads. Never again: the point is that they come from an older build.
#[tokio::test]
#[ignore = "writes the format fixtures"]
async fn make_format_fixtures() {
    let dir = Path::new(FIXTURES);
    std::fs::create_dir_all(dir).unwrap();
    let key = crate::vault::DataKey::test();
    let file = dir.join("encrypted.db");
    let _ = std::fs::remove_file(&file);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&file)
                .pragma("key", key_pragma(&key))
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::query("CREATE TABLE t(x TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO t VALUES ('unstrung')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    std::fs::write(
        dir.join("sealed.bin"),
        key.seal(b"unstrung picture").unwrap(),
    )
    .unwrap();
}

/// SQLCipher or the picture format must not change under existing users:
/// files made by an earlier build open with the same key.
#[tokio::test]
async fn files_from_an_earlier_build_still_open() {
    let key = crate::vault::DataKey::test();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("encrypted.db");
    std::fs::copy(Path::new(FIXTURES).join("encrypted.db"), &file).unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(file_options(&file, &key).read_only(true))
        .await
        .unwrap();
    let x: String = sqlx::query_scalar("SELECT x FROM t")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(x, "unstrung");
    pool.close().await;
    let sealed = std::fs::read(Path::new(FIXTURES).join("sealed.bin")).unwrap();
    assert_eq!(key.open(&sealed).unwrap(), b"unstrung picture");
}

/// A new install never writes a plain chats file: the first open makes it
/// encrypted, and the starters' pictures are sealed.
#[cfg(windows)]
#[tokio::test]
async fn a_new_install_stores_everything_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    paths.ensure().unwrap();
    let key = paths.key().unwrap();
    let db = Db::open(&paths.database(), &key).await.unwrap();
    crate::starters::ensure(&db, &paths).await.unwrap();
    db.close().await;
    assert!(!crate::vault::is_plain_database(&paths.database()));
    assert!(!readable_without_key(&paths.database()).await);
    let avatars: Vec<_> = std::fs::read_dir(paths.avatars())
        .unwrap()
        .flatten()
        .collect();
    assert!(!avatars.is_empty());
    for a in avatars {
        assert!(
            crate::vault::is_sealed(&std::fs::read(a.path()).unwrap()),
            "{:?}",
            a.path()
        );
    }
}

#[tokio::test]
async fn stats_count_live_chats_and_hide_adult_characters() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let ren_id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let adult = CharacterFields {
        name: "Vex".into(),
        is_adult: true,
        ..ren()
    };
    let vex_id = db.insert_character(&adult, None, "{}").await.unwrap();
    let ren_c = db.character(ren_id).await.unwrap();
    let chat = db.create_chat(&ren_c, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let hello = user(&db, chat, Some(greeting), "Hello").await;
    let gone = user(&db, chat, Some(hello.id), "Gone").await;
    db.delete_subtree(chat, gone.id, 1).await.unwrap();
    // A deleted chat does not count.
    let old = db.create_chat(&ren_c, None, "", true).await.unwrap();
    db.delete_chat(old).await.unwrap();
    let vex_c = db.character(vex_id).await.unwrap();
    db.create_chat(&vex_c, None, "", true).await.unwrap();

    let shown = db.stats(false).await.unwrap();
    assert_eq!(
        shown,
        [CharacterStats {
            character_id: ren_id,
            name: "Ren".into(),
            chats: 1,
            messages: 2,
            pictures: 0,
        }]
    );
    let all = db.stats(true).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!((all[1].name.as_str(), all[1].messages), ("Vex", 1));
}

#[tokio::test]
async fn a_transcript_shows_the_visible_branch_with_names() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    user(&db, chat, Some(greeting), "Other branch").await;
    let hello = user(&db, chat, Some(greeting), "Hello").await;
    let gone = user(&db, chat, Some(hello.id), "Gone").await;
    db.delete_subtree(chat, gone.id, 1).await.unwrap();
    db.set_chat_head(chat, Some(hello.id)).await.unwrap();

    let t = db.transcript(chat).await.unwrap();
    assert_eq!(t.file_name, "Chat with Ren.md");
    assert_eq!(t.text, "# Ren\n\n**Ren:** Sit. Talk.\n\n**User:** Hello\n");
}

#[tokio::test]
async fn search_finds_live_messages_and_treats_wildcards_as_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
        .await
        .unwrap();
    let ren_id = db.insert_character(&ren(), None, "{}").await.unwrap();
    let ren_c = db.character(ren_id).await.unwrap();
    let chat = db.create_chat(&ren_c, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let hit = user(&db, chat, Some(greeting), "The Lighthouse at dawn").await;
    user(&db, chat, Some(hit.id), "Sold at 100% off").await;
    let gone = user(&db, chat, Some(hit.id), "A lighthouse, deleted").await;
    db.delete_subtree(chat, gone.id, 1).await.unwrap();
    let old = db.create_chat(&ren_c, None, "", true).await.unwrap();
    let old_head = db.chat(old).await.unwrap().head_message_id.unwrap();
    user(&db, old, Some(old_head), "lighthouse in a deleted chat").await;
    db.delete_chat(old).await.unwrap();
    let vex = CharacterFields {
        name: "Vex".into(),
        is_adult: true,
        ..ren()
    };
    let vex_id = db.insert_character(&vex, None, "{}").await.unwrap();
    let vex_c = db.character(vex_id).await.unwrap();
    let adult_chat = db.create_chat(&vex_c, None, "", true).await.unwrap();
    let adult_head = db.chat(adult_chat).await.unwrap().head_message_id.unwrap();
    user(&db, adult_chat, Some(adult_head), "lighthouse, adult").await;

    let found = db.search("LIGHTHOUSE", false).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message_id, hit.id);
    assert_eq!(
        (
            found[0].before.as_str(),
            found[0].hit.as_str(),
            found[0].after.as_str()
        ),
        ("The ", "Lighthouse", " at dawn")
    );
    assert_eq!(db.search("lighthouse", true).await.unwrap().len(), 2);
    // % and _ are text, not wildcards.
    assert_eq!(db.search("00%", false).await.unwrap().len(), 1);
    assert!(db.search("t_e", false).await.unwrap().is_empty());
    // Too short.
    assert!(db.search(" at ", false).await.unwrap().is_empty());
    // Two Chinese or Japanese letters are a word, and one is too short.
    user(&db, chat, Some(hit.id), "明日は東京へ行く").await;
    assert_eq!(db.search("東京", false).await.unwrap().len(), 1);
    assert!(db.search("東", false).await.unwrap().is_empty());
}
