//! Adapter for the Ollama HTTP API. Only this module knows the API shape,
//! so an Ollama update changes one file.

use crate::error::{Error, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Cap on one line of the streamed reply. A normal line is under 1 KB.
const MAX_LINE_BYTES: usize = 64 * 1024;
/// Cap on the full reply text.
const MAX_REPLY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Options {
    pub temperature: f32,
    pub min_p: f32,
    pub top_p: f32,
    pub repeat_penalty: f32,
    pub repeat_last_n: i32,
    pub num_ctx: u32,
    pub num_predict: i32,
    pub stop: Vec<String>,
}

/// What to send for one chat reply.
pub struct ChatParams<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub options: &'a Options,
    pub keep_alive: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    options: &'a Options,
    keep_alive: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    #[serde(default)]
    message: Option<ChunkMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    eval_count: Option<i64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct ChatOutcome {
    pub text: String,
    /// True when the user stopped the reply.
    pub stopped: bool,
    /// True when the reply hit the token cap.
    pub cut_at_cap: bool,
    pub eval_count: Option<i64>,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    pub fn new(port: u16) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .no_proxy()
            .build()?;
        Ok(Self {
            http,
            base: format!("http://127.0.0.1:{port}"),
        })
    }

    pub async fn version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Version {
            version: String,
        }
        let v: Version = self
            .http
            .get(format!("{}/api/version", self.base))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(v.version)
    }

    pub async fn has_model(&self, name: &str) -> Result<bool> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Tag>,
        }
        #[derive(Deserialize)]
        struct Tag {
            name: String,
        }
        let tags: Tags = self
            .http
            .get(format!("{}/api/tags", self.base))
            .timeout(Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(tags.models.iter().any(|t| t.name == name))
    }

    /// Loads a model into memory. A chat request with no messages only loads the model.
    pub async fn load(&self, model: &str, keep_alive: &str) -> Result<()> {
        let body = serde_json::json!({ "model": model, "messages": [], "keep_alive": keep_alive });
        self.http
            .post(format!("{}/api/chat", self.base))
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Streams a chat reply. `on_text` gets each new piece of text.
    /// Cancel `cancel` to stop: the request is dropped, which stops the engine,
    /// and the text so far is returned with `stopped` set.
    pub async fn chat(
        &self,
        params: &ChatParams<'_>,
        reply_timeout: Duration,
        cancel: &CancellationToken,
        mut on_text: impl FnMut(&str),
    ) -> Result<ChatOutcome> {
        let request = ChatRequest {
            model: params.model,
            messages: params.messages,
            stream: true,
            options: params.options,
            keep_alive: params.keep_alive,
        };
        let send = self
            .http
            .post(format!("{}/api/chat", self.base))
            .json(&request)
            .send();
        let response = tokio::select! {
            () = cancel.cancelled() => return Ok(ChatOutcome { stopped: true, ..Default::default() }),
            r = tokio::time::timeout(reply_timeout, send) => r.map_err(|_| timeout_error(reply_timeout))??,
        };
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Engine(format!(
                "HTTP {status}: {}",
                engine_message(&body)
            )));
        }

        let mut out = ChatOutcome::default();
        let mut stream = response.bytes_stream();
        let mut line = Vec::new();
        loop {
            let next = tokio::select! {
                () = cancel.cancelled() => {
                    out.stopped = true;
                    return Ok(out);
                }
                n = tokio::time::timeout(reply_timeout, stream.next()) => n.map_err(|_| timeout_error(reply_timeout))?,
            };
            let Some(bytes) = next else {
                return Err(Error::Engine("the reply ended early".into()));
            };
            for &b in bytes?.iter() {
                if b != b'\n' {
                    if line.len() >= MAX_LINE_BYTES {
                        return Err(Error::Engine("reply line too long".into()));
                    }
                    line.push(b);
                    continue;
                }
                let finished = handle_line(&line, &mut out, &mut on_text)?;
                line.clear();
                if finished {
                    return Ok(out);
                }
            }
        }
    }
}

/// Applies one NDJSON line. Returns true on the last line.
fn handle_line(line: &[u8], out: &mut ChatOutcome, on_text: &mut impl FnMut(&str)) -> Result<bool> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    let chunk: ChatChunk = serde_json::from_slice(line)?;
    if let Some(error) = chunk.error {
        return Err(Error::Engine(error));
    }
    if let Some(message) = chunk.message
        && !message.content.is_empty()
    {
        if out.text.len() + message.content.len() > MAX_REPLY_BYTES {
            return Err(Error::Engine("reply too long".into()));
        }
        out.text.push_str(&message.content);
        on_text(&message.content);
    }
    if chunk.done {
        out.cut_at_cap = chunk.done_reason.as_deref() == Some("length");
        out.eval_count = chunk.eval_count;
        return Ok(true);
    }
    Ok(false)
}

fn timeout_error(limit: Duration) -> Error {
    Error::Timeout(limit.as_secs())
}

/// The `error` field of an Ollama error body, or the start of the body.
fn engine_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct Body {
        error: String,
    }
    match serde_json::from_str::<Body>(body) {
        Ok(b) => b.error,
        Err(_) => body.chars().take(200).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_build_the_reply() {
        let mut out = ChatOutcome::default();
        let mut seen = String::new();
        let mut push = |t: &str| seen.push_str(t);
        let lines = [
            r#"{"message":{"role":"assistant","content":"Sit."},"done":false}"#,
            "",
            r#"{"message":{"role":"assistant","content":" Talk."},"done":false}"#,
            r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"length","eval_count":7}"#,
        ];
        let mut finished = false;
        for l in lines {
            finished = handle_line(l.as_bytes(), &mut out, &mut push).unwrap();
        }
        assert!(finished);
        assert_eq!(out.text, "Sit. Talk.");
        assert_eq!(seen, "Sit. Talk.");
        assert!(out.cut_at_cap);
        assert_eq!(out.eval_count, Some(7));
    }

    #[test]
    fn engine_errors_and_bad_lines_fail() {
        let mut out = ChatOutcome::default();
        let mut push = |_: &str| {};
        assert!(handle_line(br#"{"error":"model not found"}"#, &mut out, &mut push).is_err());
        assert!(handle_line(b"not json", &mut out, &mut push).is_err());
        assert_eq!(engine_message(r#"{"error":"boom"}"#), "boom");
    }
}
