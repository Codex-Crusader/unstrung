//! Stress test: a large library and one very long chat, through the real
//! database and chat code, with a time limit for each step.
//!
//! It needs no model and no network, but it writes about a gigabyte of
//! temporary data and runs for minutes, so it is ignored by default:
//! `cargo test -p unstrung-core --release --test stress -- --ignored --nocapture`

use std::sync::Arc;
use std::time::{Duration, Instant};
use unstrung_core::chat::ChatService;
use unstrung_core::db::Db;
use unstrung_core::engine::Engine;
use unstrung_core::paths::AppPaths;
use unstrung_core::{backup, chatimport, starters};

/// The import limit: the longest chat that a user can bring in.
const LONG_CHAT: usize = 20_000;
/// The other chats: 200 chats of 500 messages.
const CHATS: usize = 200;
const PER_CHAT: usize = 500;

const WORDS: &[&str] = &[
    "the",
    "rain",
    "kept",
    "falling",
    "over",
    "the",
    "harbor",
    "café",
    "naïve",
    "smile",
    "東京",
    "lantern",
    "🙂",
    "door",
    "whispered",
    "stormlight",
    "Zoë",
    "coffee",
    "letter",
    "train",
    "midnight",
    "København",
    "quiet",
    "road",
    "∑",
    "river",
    "ghost",
    "map",
];

/// A message of `words` words, different for each `seed`.
fn text(seed: usize, words: usize) -> String {
    let mut out = String::new();
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    for i in 0..words {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if i > 0 {
            out.push(' ');
        }
        out.push_str(WORDS[x % WORDS.len()]);
    }
    out
}

/// A SillyTavern chat file with `n` messages. Every 1000th character line
/// is a long reply near the import limit.
fn jsonl(seed: usize, n: usize) -> Vec<u8> {
    let mut out = String::from("{\"user_name\":\"You\",\"character_name\":\"Ren\"}\n");
    for i in 0..n {
        let user = i % 2 == 0;
        let mes = if !user && i % 1000 == 1 {
            text(seed + i, 9_000)
        } else {
            text(seed + i, 40)
        };
        let line = serde_json::json!({
            "name": if user { "You" } else { "Ren" },
            "is_user": user,
            "mes": mes,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out.into_bytes()
}

/// Runs `f` and fails the test when it takes longer than `limit`.
async fn timed<T>(what: &str, limit: Duration, f: impl std::future::Future<Output = T>) -> T {
    let start = Instant::now();
    let out = f.await;
    let took = start.elapsed();
    println!("{what}: {took:.2?}");
    assert!(took <= limit, "{what} took {took:.2?}, more than {limit:?}");
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "writes about a gigabyte and runs for minutes"]
async fn a_large_library_stays_fast() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
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
    let characters = db.characters().await.unwrap();

    // 1. The longest chat that an import allows.
    let file = jsonl(0, LONG_CHAT);
    println!("long chat file: {} MB", file.len() / 1_000_000);
    let long = timed("import 20000 messages", Duration::from_secs(30), async {
        chatimport::import(&db, &ren, Some(kai), &file)
            .await
            .unwrap()
    })
    .await;
    assert_eq!(long.messages, LONG_CHAT);
    let chat = long.chat_id;

    // 2. Many more chats, spread over the characters.
    timed("import 200 chats of 500", Duration::from_secs(120), async {
        for c in 0..CHATS {
            let character = &characters[c % characters.len()];
            chatimport::import(&db, character, None, &jsonl(c * PER_CHAT + 7, PER_CHAT))
                .await
                .unwrap();
        }
    })
    .await;
    let total = LONG_CHAT + CHATS * PER_CHAT;
    println!("messages: {total}");

    // 3. Opening the long chat.
    let engine = Engine::new(paths.clone());
    let service = Arc::new(ChatService::new(db.clone(), engine));
    let path = timed(
        "path_to_head",
        Duration::from_secs(2),
        db.path_to_head(chat),
    )
    .await
    .unwrap();
    assert_eq!(path.len(), LONG_CHAT);
    timed("tree_links", Duration::from_secs(2), db.tree_links(chat))
        .await
        .unwrap();
    let view = timed("view", Duration::from_secs(3), service.view(chat))
        .await
        .unwrap();
    assert_eq!(view.messages.len(), LONG_CHAT);
    let json = serde_json::to_string(&view).unwrap();
    println!("view sent to the page: {} MB", json.len() / 1_000_000);
    timed(
        "open_latest",
        Duration::from_secs(3),
        service.open_latest(ren_id),
    )
    .await
    .unwrap();

    // 4. Search, the numbers and the transcript.
    for word in ["harbor", "København", "東京", "zzzz-no-hit", "🙂 door"] {
        let hits = timed(
            &format!("search {word}"),
            Duration::from_secs(3),
            db.search(word, true),
        )
        .await
        .unwrap();
        for h in &hits {
            assert!(
                h.hit.to_lowercase() == word.to_lowercase(),
                "{word}: {:?}",
                h.hit
            );
        }
    }
    for adult in [true, false] {
        timed(
            &format!("stats adult={adult}"),
            Duration::from_secs(3),
            db.stats(adult),
        )
        .await
        .unwrap();
    }
    timed(
        "recent_chats",
        Duration::from_secs(1),
        db.recent_chats(100, true),
    )
    .await
    .unwrap();
    timed(
        "chat_summaries",
        Duration::from_secs(1),
        db.chat_summaries(ren_id),
    )
    .await
    .unwrap();
    let t = timed("transcript", Duration::from_secs(5), db.transcript(chat))
        .await
        .unwrap();
    println!("transcript: {} MB", t.text.len() / 1_000_000);

    // 5. Tree changes deep in the long chat.
    let middle = path[LONG_CHAT / 2].id;
    let undo = timed(
        "delete half the chat",
        Duration::from_secs(5),
        service.delete_message(chat, middle),
    )
    .await
    .unwrap();
    timed(
        "undo the delete",
        Duration::from_secs(5),
        service.undo_delete(chat, &undo.1),
    )
    .await
    .unwrap();
    let back = db.path_to_head(chat).await.unwrap();
    assert_eq!(back.len(), LONG_CHAT, "undo brings every message back");
    let branched = timed(
        "branch at the end",
        Duration::from_secs(20),
        service.branch(chat, path[LONG_CHAT - 1].id),
    )
    .await
    .unwrap();
    assert_eq!(branched.messages.len(), LONG_CHAT);
    timed(
        "show an old branch",
        Duration::from_secs(5),
        service.show_branch(chat, path[10].id),
    )
    .await
    .unwrap();

    // 6. Reads while another task writes.
    let writer = {
        let db = db.clone();
        let ren = ren.clone();
        tokio::spawn(async move {
            for i in 0..20 {
                chatimport::import(&db, &ren, None, &jsonl(900_000 + i * 100, 100))
                    .await
                    .unwrap();
            }
        })
    };
    timed("search while writing", Duration::from_secs(30), async {
        while !writer.is_finished() {
            db.search("lantern", true).await.unwrap();
            db.recent_chats(100, true).await.unwrap();
        }
    })
    .await;
    writer.await.unwrap();

    // 7. A backup with a password, and opening it again.
    backup::set_password(&db, Some("stress test password"))
        .await
        .unwrap();
    let zip = home.path().join("stress.zip");
    let exported = timed(
        "export",
        Duration::from_secs(120),
        backup::export(&db, &paths, &zip, "stress"),
    )
    .await
    .unwrap();
    println!("backup: {} MB", exported.bytes / 1_000_000);
    let info = timed(
        "open the backup",
        Duration::from_secs(120),
        backup::open_with(&paths, &zip, Some("stress test password".into())),
    )
    .await
    .unwrap();
    assert!(info.chats >= (CHATS + 3) as i64, "{}", info.chats);
    timed(
        "add from the backup",
        Duration::from_secs(60),
        backup::add_characters(&db, &paths, &info.id),
    )
    .await
    .unwrap();
}
