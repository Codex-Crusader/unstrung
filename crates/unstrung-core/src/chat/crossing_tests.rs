use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A fake engine that streams `pieces` as one chat reply. Returns its port.
async fn engine(pieces: &'static [&'static str]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut req = vec![0u8; 65536];
        let _ = socket.read(&mut req).await;
        let mut body = String::new();
        for piece in pieces {
            let line = serde_json::json!({
                "message": { "role": "assistant", "content": piece },
                "done": false,
            });
            body.push_str(&format!("{line}\n"));
        }
        body.push_str(
            r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","eval_count":7}"#,
        );
        body.push('\n');
        let head =
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n";
        let _ = socket.write_all(head.as_bytes()).await;
        let _ = socket.write_all(body.as_bytes()).await;
    });
    port
}

async fn stream(pieces: &'static [&'static str]) -> (String, crate::ollama::ChatOutcome) {
    let client = crate::ollama::Client::new(engine(pieces).await).unwrap();
    let options = crate::ollama::Options {
        temperature: 0.8,
        min_p: 0.05,
        top_p: 1.0,
        top_k: 0,
        presence_penalty: 0.0,
        repeat_penalty: 1.0,
        repeat_last_n: 64,
        num_ctx: 4096,
        num_predict: 200,
        stop: Vec::new(),
    };
    let messages: Vec<ChatMessage> = Vec::new();
    let mut shown = String::new();
    let (outcome, _) = stream_reply(
        &client,
        &chat_params("m", &messages, &options, None),
        &CancellationToken::new(),
        "Sam",
        &[],
        &mut |p| {
            if let Progress::Text(t) = p {
                shown.push_str(t);
            }
        },
    )
    .await
    .unwrap();
    (shown, outcome)
}

#[test]
fn a_continued_reply_counts_the_text_before_it() {
    let mut line = LineWatch::new("*They have sex.*");
    assert!(line.allows(" Outside, it rains."));
    assert!(!line.allows(" Her 15-year-old sister wakes up."));
    // Once crossed, nothing more shows.
    assert!(!line.allows(" Then they sleep."));
    assert!(LineWatch::new("").allows("*They have sex.*"));
}

#[tokio::test]
async fn a_reply_with_sex_and_a_minor_never_shows_and_is_withheld() {
    let (shown, outcome) = stream(&[
        "*They kiss.* ",
        "They have sex. ",
        "Her 15-year-old sister sleeps next door. ",
        "More text.",
    ])
    .await;
    assert!(!shown.contains("15-year-old"), "{shown}");
    assert!(
        !shown.contains("More text"),
        "the stream did not stop: {shown}"
    );
    assert!(refuse_crossing(&outcome.text).is_err());

    let (shown, outcome) = stream(&["*They kiss.* ", "They have sex."]).await;
    assert!(shown.contains("They have sex."), "{shown}");
    assert!(refuse_crossing(&outcome.text).is_ok());
}
