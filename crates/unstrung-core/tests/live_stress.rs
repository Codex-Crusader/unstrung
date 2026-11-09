//! Stress test against the real chat engine: a long session of replies with
//! Stop, a second reply asked for while one streams, and a reply in a chat
//! of 20,000 imported messages.
//!
//! Needs Ollama and the default chat model, so it is ignored by default:
//! `cargo test -p unstrung-core --test live_stress -- --ignored --nocapture`

use std::sync::Arc;
use std::time::{Duration, Instant};
use unstrung_core::chat::{ChatService, RegenMode};
use unstrung_core::db::Db;
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;
use unstrung_core::{chatimport, starters};

const TURNS: usize = 30;

fn long_chat(n: usize) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..n {
        let user = i % 2 == 0;
        let mes = format!(
            "Line {i}. {} The rain on the harbor kept the lanterns low, and the letter stayed unopened.",
            if user {
                "I walk along the pier."
            } else {
                "Ren watches the water."
            }
        );
        out.push_str(
            &serde_json::json!({"name": if user {"You"} else {"Ren"}, "is_user": user, "mes": mes})
                .to_string(),
        );
        out.push('\n');
    }
    out.into_bytes()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn a_long_session_holds_up() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(
        matches!(engine.status(), EngineStatus::Ready { .. }),
        "{:?}",
        engine.status()
    );
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let ren_id = db
        .character_by_starter("ren_kagami")
        .await
        .unwrap()
        .unwrap();
    let ren = db.character(ren_id).await.unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let chat = service.new_chat(ren_id, Some(kai)).await.unwrap().chat.id;

    let mut slowest = Duration::ZERO;
    for turn in 0..TURNS {
        let start = Instant::now();
        let text = format!("Turn {turn}. Tell me one more thing about the case, briefly.");
        if turn % 5 == 4 {
            // Stop in the middle of the stream: the partial reply stays.
            let stopper = {
                let service = service.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    service.stop();
                })
            };
            let result = service.send(chat, &text, |_| {}, |_| {}).await;
            stopper.await.unwrap();
            println!(
                "turn {turn} stopped: {:?}",
                result.as_ref().map(|r| r.message.is_some())
            );
            result.unwrap();
        } else if turn % 7 == 6 {
            // A second reply while the first one streams is refused, and the first one ends well.
            let first = {
                let service = service.clone();
                let text = text.clone();
                tokio::spawn(async move { service.send(chat, &text, |_| {}, |_| {}).await })
            };
            tokio::time::sleep(Duration::from_millis(500)).await;
            let second = service.retry(chat, |_| {}).await;
            assert!(second.is_err(), "a second reply must wait");
            let view = service.view(chat).await;
            assert!(view.is_ok(), "reading the chat while it streams works");
            first.await.unwrap().unwrap();
        } else {
            let reply = service.send(chat, &text, |_| {}, |_| {}).await.unwrap();
            assert!(reply.message.is_some(), "turn {turn} has a reply");
        }
        if turn % 6 == 5 {
            let view = service.view(chat).await.unwrap();
            let last = view.messages.last().unwrap().message.id;
            service
                .regenerate(chat, last, RegenMode::Shorter, |_| {})
                .await
                .unwrap();
        }
        let took = start.elapsed();
        slowest = slowest.max(took);
        println!("turn {turn}: {took:.1?}");
    }
    let view = service.view(chat).await.unwrap();
    println!(
        "messages on the path: {}, slowest turn {slowest:.1?}",
        view.messages.len()
    );
    assert!(view.messages.len() >= 2 * TURNS, "{}", view.messages.len());
    assert!(
        slowest < Duration::from_secs(180),
        "slowest turn {slowest:.1?}"
    );

    // A reply at the end of a 20,000-message chat.
    let imported = chatimport::import(&db, &ren, Some(kai), &long_chat(20_000))
        .await
        .unwrap();
    let start = Instant::now();
    let reply = service
        .send(imported.chat_id, "What was in the letter?", |_| {}, |_| {})
        .await
        .unwrap();
    let took = start.elapsed();
    println!(
        "reply in the long chat: {took:.1?}: {:?}",
        reply.message.map(|m| m.content)
    );
    assert!(took < Duration::from_secs(180), "{took:.1?}");
    // The memory and summary work for the long chat must not block the next reply forever.
    let start = Instant::now();
    let reply = service
        .send(imported.chat_id, "And then?", |_| {}, |_| {})
        .await
        .unwrap();
    println!("second reply in the long chat: {:.1?}", start.elapsed());
    assert!(reply.message.is_some());
    service.shut_down();
}
