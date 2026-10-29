//! Phase 1 done-test against the real chat engine: a message streams a reply,
//! Stop keeps the partial text, and the history stays after a reopen.
//!
//! Needs Ollama and the default chat model, so it is ignored by default:
//! `cargo test -p unstrung-core --test live_chat -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;
use unstrung_core::chat::ChatService;
use unstrung_core::db::Db;
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;
use unstrung_core::starters;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn stream_stop_and_reopen() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();

    let engine = Engine::new(paths.clone());
    engine.start().await;
    let status = engine.status();
    assert!(matches!(status, EngineStatus::Ready { .. }), "{status:?}");

    let db = Db::open(&paths.database()).await.unwrap();
    let ren = starters::ensure_ren(&db, &paths).await.unwrap();
    assert!(
        paths
            .stored(&format!("avatars/starter-{}.png", starters::REN_KEY))
            .unwrap()
            .is_file()
    );
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let view = service.open_chat(ren).await.unwrap();
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
        .expect("a reply");
    println!("reply in {pieces} pieces: {}", reply.content);
    assert!(pieces > 1, "the reply must stream");
    assert_eq!(reply.parent_id, user_id);

    // 2. Busy while a reply runs, and Stop keeps the partial text.
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

    // 3. History after a reopen.
    db.close().await;
    let db = Db::open(&paths.database()).await.unwrap();
    let path = db.path_to_head(chat_id).await.unwrap();
    let roles: Vec<&str> = path.iter().map(|m| m.role.as_str()).collect();
    let expected: &[&str] = if stopped.is_some() {
        &["character", "user", "character", "user", "character"]
    } else {
        &["character", "user", "character", "user"]
    };
    assert_eq!(roles, expected);
    assert_eq!(path[2].content, reply.content);

    engine.stop().await;
    assert!(
        !paths.engine_pids().exists(),
        "the PID file is removed at a clean stop"
    );
}
