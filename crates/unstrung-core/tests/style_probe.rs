//! Measures how well the reply styles control reply length on a real chat.
//! Point `UNSTRUNG_PROBE_DB` at a copy of a database, never at the real one,
//! and set `UNSTRUNG_PROBE_REPLY` to a reply ID to regenerate.
//! `cargo test -p unstrung-core --test style_probe -- --ignored --nocapture`

use unstrung_core::chat::{ChatService, RegenMode};
use unstrung_core::db::Db;
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and a database copy"]
async fn style_lengths() {
    let (Ok(db_file), Ok(reply)) = (
        std::env::var("UNSTRUNG_PROBE_DB"),
        std::env::var("UNSTRUNG_PROBE_REPLY"),
    ) else {
        panic!("set UNSTRUNG_PROBE_DB and UNSTRUNG_PROBE_REPLY");
    };
    let reply: i64 = reply.parse().unwrap();
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));

    let db = Db::open(std::path::Path::new(&db_file)).await.unwrap();
    let chat_id: i64 = std::env::var("UNSTRUNG_PROBE_CHAT").map_or(1, |c| c.parse().unwrap());
    let service = ChatService::new(db, engine.clone());
    let styles = std::env::var("UNSTRUNG_PROBE_STYLES")
        .unwrap_or_else(|_| "texting,conversational,storyteller,novel".into());
    for style in styles.split(',') {
        service.set_style(chat_id, style, None).await.unwrap();
        for _ in 0..2 {
            service.show_branch(chat_id, reply).await.unwrap();
            let result = service
                .regenerate(chat_id, reply, RegenMode::Normal, |_| {})
                .await
                .unwrap();
            let text = result.message.unwrap().content;
            let words = text.split_whitespace().count();
            let paragraphs = text.split("\n\n").filter(|p| !p.trim().is_empty()).count();
            println!(
                "{style:<15} {words:>4} words {paragraphs} paragraphs | {}",
                text.replace("\n\n", " ¶ ").replace('\n', " / ")
            );
        }
    }
    engine.stop().await;
}
