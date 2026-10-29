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
    pub top_k: i32,
    pub presence_penalty: f32,
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
    /// For thinking models: `Some(false)` turns thinking off. `None` sends nothing,
    /// because a model without thinking rejects the field.
    pub think: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    options: &'a Options,
    keep_alive: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
}

impl<'a> ChatRequest<'a> {
    fn of(params: &ChatParams<'a>, stream: bool) -> Self {
        Self {
            model: params.model,
            messages: params.messages,
            stream,
            options: params.options,
            keep_alive: params.keep_alive,
            think: params.think,
        }
    }
}

/// A model that the engine has on disk.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Installed {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub details: InstalledDetails,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct InstalledDetails {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub parameter_size: String,
    #[serde(default)]
    pub quantization_level: String,
}

/// A model that the engine has in memory, and how much of it is on the GPU.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Loaded {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
}

/// One progress line of a download.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PullLine {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub error: Option<String>,
}

/// What the engine says about a model.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ModelInfo {
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl ModelInfo {
    pub fn can_think(&self) -> bool {
        self.capabilities.iter().any(|c| c == "thinking")
    }
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
    prompt_eval_count: Option<i64>,
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
    /// The real prompt size, to check the builder's estimate.
    pub prompt_eval_count: Option<i64>,
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
        Ok(self.installed().await?.iter().any(|t| t.name == name))
    }

    /// The models on disk (`GET /api/tags`).
    pub async fn installed(&self) -> Result<Vec<Installed>> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Installed>,
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
        Ok(tags.models)
    }

    /// The models in memory (`GET /api/ps`).
    pub async fn loaded(&self) -> Result<Vec<Loaded>> {
        #[derive(Deserialize)]
        struct Ps {
            models: Vec<Loaded>,
        }
        let ps: Ps = self
            .http
            .get(format!("{}/api/ps", self.base))
            .timeout(Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(ps.models)
    }

    /// The capabilities of an installed model (`POST /api/show`).
    pub async fn info(&self, model: &str) -> Result<ModelInfo> {
        let response = self
            .http
            .post(format!("{}/api/show", self.base))
            .timeout(Duration::from_secs(10))
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Engine(engine_message(&body)));
        }
        Ok(response.json().await?)
    }

    /// Removes a model from disk (`DELETE /api/delete`).
    pub async fn delete(&self, model: &str) -> Result<()> {
        let response = self
            .http
            .delete(format!("{}/api/delete", self.base))
            .timeout(Duration::from_secs(30))
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await?;
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Engine(engine_message(&body)));
        }
        Ok(())
    }

    /// Takes a model out of memory.
    pub async fn unload(&self, model: &str) -> Result<()> {
        self.http
            .post(format!("{}/api/generate", self.base))
            .timeout(Duration::from_secs(30))
            .json(&serde_json::json!({ "model": model, "keep_alive": 0 }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Downloads a model (`POST /api/pull`). The engine keeps finished parts,
    /// so a new pull after a stop or a failure continues where it was.
    /// Returns `false` when `cancel` stopped it.
    pub async fn pull(
        &self,
        model: &str,
        cancel: &CancellationToken,
        mut on_line: impl FnMut(&PullLine),
    ) -> Result<bool> {
        // The engine can stay quiet for a while when it checks a large file.
        const QUIET_LIMIT: Duration = Duration::from_secs(300);
        let send = self
            .http
            .post(format!("{}/api/pull", self.base))
            .json(&serde_json::json!({ "model": model, "stream": true }))
            .send();
        let response = tokio::select! {
            () = cancel.cancelled() => return Ok(false),
            r = tokio::time::timeout(QUIET_LIMIT, send) => r.map_err(|_| timeout_error(QUIET_LIMIT))??,
        };
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Engine(engine_message(&body)));
        }
        let mut stream = response.bytes_stream();
        let mut line = Vec::new();
        loop {
            let next = tokio::select! {
                () = cancel.cancelled() => return Ok(false),
                n = tokio::time::timeout(QUIET_LIMIT, stream.next()) => n.map_err(|_| timeout_error(QUIET_LIMIT))?,
            };
            let Some(bytes) = next else {
                return Err(Error::Engine("the download ended early".into()));
            };
            for &b in bytes?.iter() {
                if b != b'\n' {
                    if line.len() >= MAX_LINE_BYTES {
                        return Err(Error::Engine("download line too long".into()));
                    }
                    line.push(b);
                    continue;
                }
                let parsed = parse_pull_line(&line)?;
                line.clear();
                if let Some(parsed) = parsed {
                    on_line(&parsed);
                    if parsed.status == "success" {
                        return Ok(true);
                    }
                }
            }
        }
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

    /// One reply without streaming, for short helper tasks.
    pub async fn complete(&self, params: &ChatParams<'_>, timeout: Duration) -> Result<String> {
        let request = ChatRequest::of(params, false);
        let response = self
            .http
            .post(format!("{}/api/chat", self.base))
            .timeout(timeout)
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Engine(format!(
                "HTTP {status}: {}",
                engine_message(&body)
            )));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_REPLY_BYTES {
            return Err(Error::Engine("reply too long".into()));
        }
        let chunk: ChatChunk = serde_json::from_slice(&bytes)?;
        if let Some(error) = chunk.error {
            return Err(Error::Engine(error));
        }
        Ok(chunk.message.map(|m| m.content).unwrap_or_default())
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
        let request = ChatRequest::of(params, true);
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
        out.prompt_eval_count = chunk.prompt_eval_count;
        return Ok(true);
    }
    Ok(false)
}

/// Reads one download progress line. Blank lines give `None`.
fn parse_pull_line(line: &[u8]) -> Result<Option<PullLine>> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let parsed: PullLine = serde_json::from_slice(line)?;
    if let Some(error) = parsed.error {
        return Err(Error::Engine(error));
    }
    Ok(Some(parsed))
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

    #[test]
    fn pull_lines() {
        let line = parse_pull_line(br#"{"status":"pulling 6a0746a1ec1a","digest":"sha256:6a07","total":4683073184,"completed":241970}"#)
            .unwrap()
            .unwrap();
        assert_eq!((line.total, line.completed), (4_683_073_184, 241_970));
        assert!(parse_pull_line(b"  ").unwrap().is_none());
        assert!(parse_pull_line(br#"{"error":"pull model manifest: file does not exist"}"#).is_err());
        let request = ChatRequest {
            model: "m",
            messages: &[],
            stream: true,
            options: &Options {
                temperature: 0.8,
                min_p: 0.05,
                top_p: 1.0,
                top_k: 40,
                presence_penalty: 0.0,
                repeat_penalty: 1.0,
                repeat_last_n: 64,
                num_ctx: 8192,
                num_predict: 100,
                stop: vec![],
            },
            keep_alive: "5m",
            think: None,
        };
        assert!(!serde_json::to_string(&request).unwrap().contains("think"));
    }
}
