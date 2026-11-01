//! Done-tests against the real chat engine.
//!
//! Phase 1: a message streams a reply, Stop keeps the partial text, and the
//! history stays after a reopen. Phase 2: regenerate, versions, rewind, delete
//! with undo, edit, continue, branch, persona switch, OOC notes and the anchor helper.
//!
//! Needs Ollama and the default chat model, so it is ignored by default:
//! `cargo test -p unstrung-core --test live_chat -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;
use unstrung_core::chat::{ChatService, RegenMode};
use unstrung_core::db::{Db, Message};
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;
use unstrung_core::starters;

fn texts(view: &unstrung_core::chat::ChatView) -> Vec<(&str, &str)> {
    view.messages
        .iter()
        .map(|m| (m.message.role.as_str(), m.message.content.as_str()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn chat_engine_done_tests() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();

    let engine = Engine::new(paths.clone());
    engine.start().await;
    let status = engine.status();
    assert!(matches!(status, EngineStatus::Ready { .. }), "{status:?}");

    let db = Db::open(&paths.database()).await.unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let ren = db
        .character_by_starter("ren_kagami")
        .await
        .unwrap()
        .unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let nora = db.persona_by_starter("nora").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let view = service.new_chat(ren, Some(kai)).await.unwrap();
    assert_eq!(view.messages.len(), 1, "the chat starts with the greeting");
    let chat_id = view.chat.id;

    // 1. A full streamed reply.
    let mut pieces = 0;
    let mut user_id = None;
    let reply = service
        .send(
            chat_id,
            "I have a case for you. My brother is missing.",
            |m| user_id = Some(m.id),
            |_| pieces += 1,
        )
        .await
        .unwrap()
        .message
        .expect("a reply");
    println!("reply in {pieces} pieces: {}", reply.content);
    assert!(pieces > 1, "the reply must stream");
    assert_eq!(reply.parent_id, user_id);

    // 2. Regenerate keeps the old reply as a sibling, and the arrows switch between them.
    let shorter = service
        .regenerate(chat_id, reply.id, RegenMode::Shorter, |_| {})
        .await
        .unwrap()
        .message
        .unwrap();
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.messages[2].siblings, [reply.id, shorter.id]);
    let view = service.show_branch(chat_id, reply.id).await.unwrap();
    assert_eq!(view.messages[2].message.id, reply.id);
    service
        .regenerate(chat_id, reply.id, RegenMode::NotInCharacter, |_| {})
        .await
        .unwrap();
    assert!(
        db.message(chat_id, reply.id)
            .await
            .unwrap()
            .flagged_out_of_character
    );
    service.show_branch(chat_id, reply.id).await.unwrap();

    // 3. Continue adds to the same message.
    let longer = service
        .continue_reply(chat_id, |_| {})
        .await
        .unwrap()
        .message
        .unwrap();
    assert_eq!(longer.id, reply.id);
    assert!(longer.content.len() >= reply.content.len());
    println!("continued: {}", longer.content.replace('\n', " / "));

    // 4. Persona switch adds a note, an OOC note works, and a rewind restores Kai.
    let view = service.set_persona(chat_id, Some(nora)).await.unwrap();
    assert_eq!(view.messages.last().unwrap().message.role, "system");
    let ooc = service
        .send(chat_id, "/ooc keep it short and moody", |_| {}, |_| {})
        .await
        .unwrap();
    assert!(ooc.message.is_some());
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.persona.as_ref().unwrap().name, "Nora");
    let ooc_msg = &view.messages[view.messages.len() - 2].message;
    assert_eq!(ooc_msg.role, "ooc");
    let view = service.set_head(chat_id, reply.id).await.unwrap();
    assert_eq!(view.persona.as_ref().unwrap().name, "Kai");
    assert_eq!(view.branches.len(), 1, "the system note is a hidden branch");

    // 5. Edit a user message: a new version and a new reply.
    let edited = service
        .edit_and_resend(
            chat_id,
            user_id.unwrap(),
            "My sister is missing.",
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
    assert!(edited.message.is_some());
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.messages[1].message.content, "My sister is missing.");
    assert_eq!(view.messages[1].siblings.len(), 2);

    // 6. Delete with undo.
    let before = texts(&view).len();
    let target = view.messages[1].message.id;
    let (after, undo) = service.delete_message(chat_id, target).await.unwrap();
    assert_eq!(texts(&after).len(), 1);
    let restored = service.undo_delete(chat_id, &undo).await.unwrap();
    assert_eq!(texts(&restored).len(), before);

    // 7. Branch to a new chat copies the path up to a message.
    let branch = service.branch(chat_id, target).await.unwrap();
    assert_ne!(branch.chat.id, chat_id);
    assert_eq!(texts(&branch), texts(&restored)[..2]);

    // 8. Busy while a reply runs, and Stop keeps the partial text.
    let (first_tx, first_rx) = tokio::sync::oneshot::channel();
    let runner = {
        let service = service.clone();
        tokio::spawn(async move {
            let mut first = Some(first_tx);
            service
                .send(
                    chat_id,
                    "Tell me everything you know about this city, in detail.",
                    |_| {},
                    |_| {
                        if let Some(tx) = first.take() {
                            let _ = tx.send(());
                        }
                    },
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(60), first_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        service.retry(chat_id, |_| {}).await,
        Err(unstrung_core::Error::Busy)
    ));
    service.stop();
    let stopped = runner.await.unwrap().unwrap();
    println!("stopped reply: {stopped:?}");

    // 9. The voice anchor helper.
    let anchor = service
        .suggest_anchor("June", "Warm, chatty, a bit clumsy.")
        .await
        .unwrap();
    println!("anchor: {anchor}");
    assert!(!anchor.is_empty() && anchor.len() <= 400);

    // 10. History after a reopen.
    let expected: Vec<(String, String)> = texts(&service.view(chat_id).await.unwrap())
        .into_iter()
        .map(|(r, c)| (r.to_owned(), c.to_owned()))
        .collect();
    db.close().await;
    let db = Db::open(&paths.database()).await.unwrap();
    let path: Vec<Message> = db.path_to_head(chat_id).await.unwrap();
    let got: Vec<(String, String)> = path.into_iter().map(|m| (m.role, m.content)).collect();
    assert_eq!(got, expected);

    engine.stop().await;
    assert!(
        !paths.engine_pids().exists(),
        "the PID file is removed at a clean stop"
    );
}

/// Phase 3: a missing default model leaves the engine running, so the model
/// list works. A chat cannot reply then. Picking an installed model makes it ready.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn missing_model_keeps_the_engine_running() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();

    let engine = Engine::with_model(
        paths.clone(),
        "no-such-model:1b",
        unstrung_core::catalog::Advanced::default(),
    );
    engine.start().await;
    let status = engine.status();
    assert!(matches!(status, EngineStatus::NoModel { .. }), "{status:?}");
    assert!(engine.client().is_err());
    let installed = engine.refresh_installed().await.unwrap();
    assert!(engine.is_installed(unstrung_core::engine::DEFAULT_CHAT_MODEL));
    assert!(!installed.is_empty());

    engine
        .set_model(unstrung_core::engine::DEFAULT_CHAT_MODEL)
        .await
        .unwrap();
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    assert!(engine.client().is_ok());
    engine.stop().await;
}

/// Phase 4: a GGUF file imports as a model of its own. Set UNSTRUNG_TEST_GGUF
/// to a GGUF file. By default the file is a hard link, so the engine already
/// has the blob and skips the upload. With UNSTRUNG_TEST_GGUF_UPLOAD set, the
/// test copies the file with one more byte, so the upload runs too.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama, the test models and UNSTRUNG_TEST_GGUF"]
async fn a_gguf_file_imports_as_a_model() {
    let source = std::path::PathBuf::from(std::env::var("UNSTRUNG_TEST_GGUF").unwrap());
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let file = home.path().join("My Test Model.gguf");
    if std::env::var_os("UNSTRUNG_TEST_GGUF_UPLOAD").is_some() {
        // One byte more gives a new digest, so the engine must receive the
        // whole file. GGUF readers ignore bytes after the last tensor.
        std::fs::copy(&source, &file).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        std::io::Write::write_all(&mut f, &[0]).unwrap();
    } else {
        std::fs::hard_link(&source, &file).unwrap();
    }

    let engine = Engine::new(paths.clone());
    engine.start().await;
    let db = Db::open(&paths.database()).await.unwrap();
    let startup = unstrung_core::models::startup(&db, &paths).await.unwrap();
    let models = unstrung_core::models::ModelService::new(engine.clone(), db, &startup);
    let mut progress = models.subscribe();

    let name = models.import_file(file.clone()).await.unwrap();
    assert_eq!(name, "my-test-model");
    let end = loop {
        progress.changed().await.unwrap();
        let now = progress.borrow_and_update().clone().unwrap();
        if !matches!(now.state, unstrung_core::models::DownloadState::Importing) {
            break now;
        }
    };
    assert_eq!(
        end.state,
        unstrung_core::models::DownloadState::Done,
        "{end:?}"
    );
    let installed = engine.refresh_installed().await.unwrap();
    assert!(
        installed
            .iter()
            .any(|m| m.name.starts_with("my-test-model"))
    );

    // A file with a .gguf name but other bytes is refused.
    let not_gguf = home.path().join("notes.gguf");
    std::fs::write(&not_gguf, b"not a model").unwrap();
    assert!(models.import_file(not_gguf).await.is_err());

    engine
        .client()
        .unwrap()
        .delete("my-test-model")
        .await
        .unwrap();
    engine.stop().await;
    // The database pool can still hold app.db, which stops the temporary
    // folder from going away on Windows, so remove the large file now.
    std::fs::remove_file(&file).unwrap();
}

/// The scene writer fills a person's outfit only when the story changed
/// their clothes. Prints each answer, so a person can read them.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn the_scene_writer_gives_clothes_only_when_the_story_changes_them() {
    use unstrung_core::db::NewMessage;
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let db = Db::open(&paths.database()).await.unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let mara = db.character_by_starter("mara_voss").await.unwrap().unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));

    let chat = |lines: &'static [(&'static str, &'static str)]| {
        let service = service.clone();
        let db = db.clone();
        async move {
            let view = service.new_chat(mara, Some(kai)).await.unwrap();
            let mut parent = view.messages.last().map(|m| m.message.id);
            for (role, text) in lines {
                let m = db
                    .append_message(&NewMessage {
                        chat_id: view.chat.id,
                        parent_id: parent,
                        role,
                        persona_id: Some(kai),
                        content: text,
                        raw_content: None,
                        token_count: None,
                        model: None,
                        prompt_version: None,
                        sampler_json: None,
                    })
                    .await
                    .unwrap();
                parent = Some(m.id);
            }
            view.chat.id
        }
    };
    let gala = chat(&[
        ("user", "The gala starts in an hour. You cannot go in that flight jacket, Captain."),
        ("character", "*Mara sighs, goes into her cabin and comes back in a long red evening dress.* Happy now? Do not get used to it."),
        ("user", "You look amazing. Shall we go?"),
    ])
    .await;
    let engine_room = chat(&[
        ("user", "I think the coupling is shot."),
        (
            "character",
            "*She taps the cracked console.* Then we make a new one. Grab a wrench.",
        ),
        ("user", "On it. Hold the light for me?"),
    ])
    .await;

    let token = tokio_util::sync::CancellationToken::new();
    let mut changed = 0;
    let mut invented = 0;
    for _ in 0..3 {
        let s = service
            .picture_scene(gala, None, "", &token)
            .await
            .unwrap()
            .scene;
        println!(
            "gala: character {:?}, persona {:?}",
            s.character_outfit, s.persona_outfit
        );
        if s.character_outfit.contains("dress") {
            changed += 1;
        }
        let s = service
            .picture_scene(engine_room, None, "", &token)
            .await
            .unwrap()
            .scene;
        println!(
            "engine room: character {:?}, persona {:?}",
            s.character_outfit, s.persona_outfit
        );
        if !s.character_outfit.is_empty() || !s.persona_outfit.is_empty() {
            invented += 1;
        }
    }
    engine.stop().await;
    assert!(changed >= 2, "the dress showed {changed} of 3 times");
    assert!(invented <= 1, "clothes were made up {invented} of 3 times");
}
