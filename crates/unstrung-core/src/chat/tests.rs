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

#[tokio::test]
async fn growth_off_asks_the_model_nothing() {
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
    for role in ["user", "character"] {
        head = db
            .append_message(&NewMessage {
                chat_id: chat,
                parent_id: Some(head),
                role,
                content: "line",
                ..Default::default()
            })
            .await
            .unwrap()
            .id;
    }
    db.set_chat_head(chat, Some(head)).await.unwrap();
    // The engine never started: any model call would fail.
    let chats = ChatService::new(db, Engine::new(paths));
    assert!(chats.appraise(chat).await.unwrap().is_none());
    // On, the same call needs the engine.
    chats
        .db()
        .set_meta(crate::growth::ask::SWITCH_KEY, "on")
        .await
        .unwrap();
    assert!(chats.appraise(chat).await.is_err());
    assert!(chats.db().growth_states(chat).await.unwrap().is_empty());
}

/// A fake engine that answers each request with the next text of `answers`
/// as one finished chat message. Returns its port and the request count.
async fn answering_engine(
    answers: Vec<&'static str>,
) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = count.clone();
    tokio::spawn(async move {
        for answer in answers {
            let (mut socket, _) = listener.accept().await.unwrap();
            // Reads the whole request: the headers, then Content-Length bytes.
            let mut req = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                req.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&req).to_lowercase();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if req.len() >= end + 4 + len || n == 0 {
                        break;
                    }
                }
            }
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = serde_json::json!({
                "message": { "role": "assistant", "content": answer },
                "done": true, "done_reason": "stop", "eval_count": 30,
                "prompt_eval_count": 250, "prompt_eval_duration": 90_000_000,
            })
            .to_string()
                + "\n";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body.as_bytes()).await.unwrap();
        }
    });
    (port, count)
}

#[tokio::test]
async fn a_reply_is_appraised_once_and_a_broken_answer_changes_no_drive() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::AppPaths::at(dir.path().to_path_buf());
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let fields = crate::db::CharacterFields {
        name: "Ren".into(),
        personality: "Proud and slow to forgive.".into(),
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
    let add = |parent: i64, role: &'static str| {
        let db = db.clone();
        async move {
            db.append_message(&NewMessage {
                chat_id: chat,
                parent_id: Some(parent),
                role,
                content: "line",
                ..Default::default()
            })
            .await
            .unwrap()
            .id
        }
    };
    let said = add(greeting, "user").await;
    let first = add(said, "character").await;
    db.set_chat_head(chat, Some(first)).await.unwrap();
    db.set_meta(crate::growth::ask::SWITCH_KEY, "on")
        .await
        .unwrap();

    let (port, requests) = answering_engine(vec![
        r#"{"warmth":2,"sensitivity":4,"grudges":5,"fairness":4,"forgiveness":1}"#,
        r#"{"1_deed":"sold the secret","2_event":true,"valence":-2,"unfair":true,"apology":false,"trust":-2,"closeness":-2,"respect":-1,"romance":0}"#,
        "Trust drops a lot here, I think.",
    ])
    .await;
    let engine = Engine::new(paths);
    engine.use_test_client(crate::ollama::Client::new(port).unwrap());
    let chats = ChatService::new(db.clone(), engine);
    let n = || requests.load(std::sync::atomic::Ordering::SeqCst);

    // The genome question, then the critic.
    let cost = chats.appraise(chat).await.unwrap().unwrap();
    assert!(cost.parsed && !cost.continued);
    assert_eq!(cost.prompt_tokens, Some(250));
    assert_eq!(n(), 2);
    let genome = chats.db().genome(id).await.unwrap().unwrap().1;
    let rows = chats.db().growth_states(chat).await.unwrap();
    let start = crate::growth::State::new(&genome);
    let hurt = rows[0].1.clone();
    assert!(hurt.drives.trust < start.drives.trust);
    assert!(hurt.drives.closeness < start.drives.closeness);

    // Asking again, as a retry or a second window would, asks nothing.
    for _ in 0..5 {
        assert!(chats.appraise(chat).await.unwrap().is_none());
    }
    assert_eq!(n(), 2);

    // A regenerated reply is a new message: appraised from the state before
    // the old one, once. The answer is prose, so no drive moves.
    let second = add(said, "character").await;
    chats.db().set_chat_head(chat, Some(second)).await.unwrap();
    let cost = chats.appraise(chat).await.unwrap().unwrap();
    assert!(!cost.parsed);
    assert_eq!(n(), 3, "the genome is asked for once per card");
    let rows = chats.db().growth_states(chat).await.unwrap();
    let after = &rows.iter().find(|r| r.0 == second).unwrap().1;
    assert_eq!(after.drives, start.drives);
    assert!(chats.appraise(chat).await.unwrap().is_none());
    assert_eq!(n(), 3);
}
