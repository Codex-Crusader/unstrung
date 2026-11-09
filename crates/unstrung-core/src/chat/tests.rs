use super::*;

#[tokio::test]
async fn a_rebuilt_summary_saves_only_while_it_is_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let fields = crate::db::CharacterFields {
        name: "Ren".into(),
        greeting: "Sit.".into(),
        ..Default::default()
    };
    let id = db
        .insert_character(&fields, Some("test"), "{}")
        .await
        .unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let mut head = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let mut ids = vec![head];
    for i in 0..6 {
        let m = db
            .append_message(&NewMessage {
                chat_id: chat,
                parent_id: Some(head),
                role: if i % 2 == 0 { "user" } else { "character" },
                content: "line",
                ..Default::default()
            })
            .await
            .unwrap();
        head = m.id;
        ids.push(head);
    }
    db.add_memory(chat, "summary", "Old story.", ids[2], "auto")
        .await
        .unwrap();
    let chats = ChatService::new(db, Engine::new(paths));

    // Covers up to message 4: newer than the old summary, so it wins.
    let list = chats
        .save_summary(chat, ids[4], " The *new* story. ")
        .await
        .unwrap();
    let summary = list.iter().find(|m| m.kind == "summary").unwrap();
    assert_eq!(summary.content, "The new story.");
    assert_eq!(summary.origin, "user");
    // A rebuild up to message 3 is older than the saved one: out of date.
    assert!(matches!(
        chats.save_summary(chat, ids[3], "Late.").await,
        Err(Error::Invalid(_))
    ));
    // A message that is not on the path: out of date.
    assert!(matches!(
        chats.save_summary(chat, 9999, "Gone.").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        chats.save_summary(chat, ids[5], " ** ").await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn a_next_chapter_carries_the_notes_and_the_settings() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let fields = crate::db::CharacterFields {
        name: "Ren".into(),
        greeting: "Sit.".into(),
        ..Default::default()
    };
    let id = db
        .insert_character(&fields, Some("test"), "{}")
        .await
        .unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    db.set_chat_tone(chat, "dark", "").await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let hello = db
        .append_message(&NewMessage {
            chat_id: chat,
            parent_id: Some(greeting),
            role: "user",
            content: "Hello",
            ..Default::default()
        })
        .await
        .unwrap();
    db.add_memory(chat, "summary", "Kai hired Ren.", hello.id, "auto")
        .await
        .unwrap();
    db.add_memory(chat, "goal", "Find the watch.", hello.id, "auto")
        .await
        .unwrap();
    let pinned = db
        .add_memory(chat, "fact", "Kai has a sister.", hello.id, "user")
        .await
        .unwrap();
    db.set_memory_flags(chat, pinned.id, true, false)
        .await
        .unwrap();
    let hidden = db
        .add_memory(chat, "fact", "Kai is a spy.", hello.id, "auto")
        .await
        .unwrap();
    db.set_memory_flags(chat, hidden.id, false, true)
        .await
        .unwrap();
    let chats = ChatService::new(db, Engine::new(paths));

    let view = chats.next_chapter(chat).await.unwrap();
    let next = view.chat.id;
    assert_eq!(view.chat.title, "Chapter 2");
    assert_eq!(view.chat.tone, "dark");
    assert_eq!(view.messages.len(), 1);
    assert_eq!(view.messages[0].message.role, "system");
    let path = chats.db().path_to_head(next).await.unwrap();
    let memories = chats.db().memories(next).await.unwrap();
    let recall = memory::recall(&memories, &path);
    assert_eq!(recall.summary.as_deref(), Some("Kai hired Ren."));
    assert_eq!(recall.goal.as_deref(), Some("Find the watch."));
    assert_eq!(
        recall.facts,
        ["Kai has a sister."],
        "the hidden fact stays behind"
    );
    assert!(memories.iter().any(|m| m.is_pinned && m.origin == "user"));
    // The memory pass starts after the note: nothing to read yet.
    assert!(memory::pending(&memories, &path).is_none());
    // The old chat stays, and a third chapter counts on.
    assert!(chats.db().chat(chat).await.is_ok());
    assert_eq!(
        chats.next_chapter(next).await.unwrap().chat.title,
        "Chapter 3"
    );
}

#[tokio::test]
async fn memory_panel_calls() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let fields = crate::db::CharacterFields {
        name: "Ren".into(),
        greeting: "Sit.".into(),
        ..Default::default()
    };
    let id = db
        .insert_character(&fields, Some("test"), "{}")
        .await
        .unwrap();
    let character = db.character(id).await.unwrap();
    let chat = db.create_chat(&character, None, "", true).await.unwrap();
    let greeting = db.chat(chat).await.unwrap().head_message_id.unwrap();
    let new = |parent, content| NewMessage {
        chat_id: chat,
        parent_id: Some(parent),
        role: "user",
        content,
        ..Default::default()
    };
    let hello = db.append_message(&new(greeting, "Hello")).await.unwrap();
    // A sibling that is not on the path to the head.
    let other = db.append_message(&new(greeting, "Other")).await.unwrap();
    db.set_chat_head(chat, Some(hello.id)).await.unwrap();
    let chats = ChatService::new(db, Engine::new(paths));

    let list = chats
        .remember(chat, hello.id, " My sister is *Emi* ")
        .await
        .unwrap();
    assert_eq!(list[0].content, "My sister is Emi");
    assert_eq!(list[0].origin, "user");
    assert!(matches!(
        chats.remember(chat, other.id, "x").await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        chats.remember(chat, hello.id, " ** ").await,
        Err(Error::Invalid(_))
    ));

    let fact = list[0].id;
    let list = chats
        .set_memory_flags(chat, fact, true, false)
        .await
        .unwrap();
    assert!(list[0].is_pinned);
    let summary = chats
        .db
        .add_memory(chat, "summary", "Ren met Kai.", hello.id, "auto")
        .await
        .unwrap();
    let list = chats
        .edit_memory(chat, summary.id, "Ren met Kai at the bar.")
        .await
        .unwrap();
    let summary = list.iter().find(|m| m.kind == "summary").unwrap();
    assert_eq!(
        (summary.content.as_str(), summary.origin.as_str()),
        ("Ren met Kai at the bar.", "user")
    );
    // Only a fact can be pinned.
    let list = chats
        .set_memory_flags(chat, summary.id, true, false)
        .await
        .unwrap();
    assert!(!list.iter().find(|m| m.kind == "summary").unwrap().is_pinned);

    // A regenerate replaces the reply, not the user message, so a fact on
    // the user message stays. This is why the panel uses it as the anchor.
    let reply = |content| NewMessage {
        chat_id: chat,
        parent_id: Some(hello.id),
        role: "character",
        content,
        ..Default::default()
    };
    chats
        .db
        .append_message(&reply("First reply."))
        .await
        .unwrap();
    let again = chats
        .db
        .append_message(&reply("Second reply."))
        .await
        .unwrap();
    chats.db.set_chat_head(chat, Some(again.id)).await.unwrap();
    assert!(
        chats
            .memories(chat)
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == fact)
    );
}

#[test]
fn input_is_checked() {
    assert!(clean_input("   ").is_err());
    assert!(clean_input(&"a".repeat(MAX_MESSAGE_CHARS + 1)).is_err());
    assert!(clean_input("*He undressed the schoolgirl.*").is_err());
    assert!(clean_input("*She pulls him onto the bed.*").is_ok());
    assert_eq!(
        clean_input(" hi\u{0007}\nthere\u{202E} ").unwrap(),
        "hi\nthere"
    );
    assert_eq!(clean_line("a\nb\u{202E}c", 10), "a bc");
}

#[test]
fn continuation_join() {
    assert_eq!(
        join_continuation("The case", " started."),
        "The case started."
    );
    assert_eq!(join_continuation("The case", "s"), "The cases");
    assert_eq!(join_continuation("Done.", "More."), "Done.\n\nMore.");
    assert_eq!(join_continuation("Done.", ""), "Done.");
}

#[test]
fn ooc_notes() {
    assert_eq!(
        split_ooc("/ooc make it darker").unwrap(),
        ("ooc", "make it darker".into())
    );
    assert_eq!(split_ooc("/OOC  x").unwrap(), ("ooc", "x".into()));
    assert_eq!(
        split_ooc("(( slower pace ))").unwrap(),
        ("ooc", "slower pace".into())
    );
    assert_eq!(
        split_ooc("hello (( x").unwrap(),
        ("user", "hello (( x".into())
    );
    assert_eq!(split_ooc("/oocx").unwrap(), ("user", "/oocx".into()));
    assert!(split_ooc("/ooc").is_err());
    assert!(split_ooc("(())").is_err());
}
