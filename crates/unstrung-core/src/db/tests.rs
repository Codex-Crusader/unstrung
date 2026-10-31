use super::*;

fn ren() -> CharacterFields {
    CharacterFields {
        name: "Ren".into(),
        personality: "Tired.".into(),
        greeting: "Sit. Talk.".into(),
        default_tone: "dark".into(),
        default_reply_style: "conversational".into(),
        default_reply_custom: String::new(),
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
        Some("4")
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
