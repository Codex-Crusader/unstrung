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
    let db = Db::open(&file).await.unwrap();
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

    let db = Db::open(&file).await.unwrap();
    let path = db.path_to_head(chat).await.unwrap();
    assert_eq!(texts(&path), ["Sit. Talk.", "Hello", "Talk."]);
    assert_eq!(db.latest_chat(id).await.unwrap(), Some(chat));
    assert_eq!(
        db.meta("schema_version").await.unwrap().as_deref(),
        Some("7")
    );
    let summaries = db.chat_summaries(id).await.unwrap();
    assert_eq!(summaries[0].message_count, 4);
    assert_eq!(summaries[0].preview, "Hello");
}

#[tokio::test]
async fn delete_restore_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db")).await.unwrap();
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
    let db = Db::open(&dir.path().join("app.db")).await.unwrap();
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
    let db = Db::open(&file).await.unwrap();
    assert!(db.first_run_pending().await.unwrap());
    db.close().await;
    // Closing the app during the welcome flow keeps it pending.
    let db = Db::open(&file).await.unwrap();
    assert!(db.first_run_pending().await.unwrap());
    db.set_meta(FIRST_RUN_KEY, "done").await.unwrap();
    db.close().await;
    let db = Db::open(&file).await.unwrap();
    assert!(!db.first_run_pending().await.unwrap());
}

#[tokio::test]
async fn a_database_from_before_the_welcome_flow_skips_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("app.db");
    let db = Db::open(&file).await.unwrap();
    // An older build had no first-run flag.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(FIRST_RUN_KEY)
        .execute(&db.pool)
        .await
        .unwrap();
    db.close().await;
    let db = Db::open(&file).await.unwrap();
    assert_eq!(
        db.meta(FIRST_RUN_KEY).await.unwrap().as_deref(),
        Some("existing")
    );
    assert!(!db.first_run_pending().await.unwrap());
}

#[tokio::test]
async fn recent_chats_are_talked_in_newest_first_and_not_adult() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("app.db")).await.unwrap();
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

    let recent = db.recent_chats(8).await.unwrap();
    let ids: Vec<i64> = recent.iter().map(|r| r.chat_id).collect();
    assert_eq!(ids, [newer, older]);
    assert_eq!(recent[0].preview, "Where were we?");
    assert_eq!(recent[0].character_name, "Ren");
    assert_eq!(db.recent_chats(1).await.unwrap().len(), 1);
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
    Db::open(&file).await.unwrap().close().await;
    let made = copies(dir.path());
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].starts_with("before-schema-3-"), "{made:?}");
    let copy = dir.path().join("backups").join(&made[0]);
    assert_eq!(
        Db::integrity_check_file(&copy).await.unwrap(),
        Some(Vec::new())
    );
    // Opening again changes nothing, so it makes no second copy.
    Db::open(&file).await.unwrap().close().await;
    assert_eq!(copies(dir.path()).len(), 1);
}

#[tokio::test]
async fn a_new_database_makes_no_copy() {
    let dir = tempfile::tempdir().unwrap();
    Db::open(&dir.path().join("app.db"))
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
    let db = Db::open(&file).await.unwrap();
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
    let db = Db::open(&dir.path().join("app.db")).await.unwrap();
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
    let gallery = db.gallery().await.unwrap();
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
    assert!(db.gallery().await.unwrap().is_empty());
    assert_eq!(
        db.character_image_files(id).await.unwrap(),
        [format!("{chat}/b.png")]
    );
    // Deleting the character removes the rows.
    db.delete_character(id).await.unwrap();
    assert!(db.chat_images(chat).await.unwrap().is_empty());
}
