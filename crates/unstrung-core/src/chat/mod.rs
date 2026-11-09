//! Chat logic: replies, the message tree, personas, tone and reply style.
//!
//! Messages form a tree. Rewind, regenerate, edit and branch all move the head
//! of the chat, so later messages become hidden branches and nothing is lost.

use crate::db::{Character, Chat, ChatSummary, Db, Memory, Message, NewMessage, Persona, now_ms};
use crate::engine::{Engine, KEEP_ALIVE};
use crate::error::{Error, Result};
use crate::guard::{self, Problem};
use crate::memory;
use crate::ollama::{ChatMessage, ChatParams, Options};
use crate::pictures;
use crate::presets::{self, StyleSettings};
use crate::prompt::{self, PROMPT_VERSION, PromptInput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

mod assist;
#[cfg(test)]
mod crossing_tests;
mod input;
mod memories;
mod reply;
#[cfg(test)]
mod tests;
mod views;

pub use input::*;
use reply::*;
use views::*;

/// Longest user message, in characters.
pub const MAX_MESSAGE_CHARS: usize = 8000;
/// "No reply after 60 seconds."
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// The open-thread question is short. A slow answer is skipped.
const THREAD_TIMEOUT: Duration = Duration::from_secs(20);
/// The open thread shows only after a user message this short or shorter.
const THREAD_SHORT_WORDS: usize = 8;
/// A memory pass reads up to 10 messages and writes a short summary and facts.
const MEMORY_TOKENS: i32 = 400;
const MEMORY_TIMEOUT: Duration = Duration::from_secs(90);
/// The scene JSON is short.
const SCENE_TOKENS: i32 = 300;
const SCENE_TIMEOUT: Duration = Duration::from_secs(60);
/// Open-thread answers kept in memory before the cache starts again.
const THREAD_CACHE: usize = 1000;
/// The end-reminder line for "Continue".
const CONTINUE_REMINDER: &str =
    "Continue your last reply with one more paragraph. Do not repeat what you already wrote.";
/// Shown under a reply that failed the checks twice.
pub const OUT_OF_CHARACTER_HINT: &str = "Not in character? Regenerate.";
/// The hint when the retry still repeats an earlier reply.
pub const REPEAT_HINT: &str = "Repeating itself? Regenerate.";

/// The error for a reply with no text left. When the model wrote text and the
/// guard removed all of it, the text was lines for the user.
/// Shown when `refuse_crossing` withholds a reply.
const CROSSED_MESSAGE: &str =
    "This reply was not shown. Unstrung never shows sexual content that involves anyone under 18.";

/// Withholds a reply that crosses the hard limit, whatever the adult
/// setting. It is not saved, so the model does not see it in the next
/// prompt. `raw` is the model's own text, before the guard cut anything.
fn refuse_crossing(raw: &str) -> Result<()> {
    if !crate::pictures::safety::reply_crosses_line(raw) {
        return Ok(());
    }
    log::warn!("a reply was withheld: explicit words next to a minor");
    Err(Error::Rejected {
        message: CROSSED_MESSAGE.into(),
        why: "explicit words next to a minor".into(),
    })
}

/// The reply text as it streams, for the hard limit: the stream stops before
/// a piece that crosses the line shows, and `refuse_crossing` then withholds
/// the reply. `base` is the text a continued reply goes on from.
struct LineWatch {
    seen: String,
    crossed: bool,
}

impl LineWatch {
    fn new(base: &str) -> Self {
        Self {
            seen: base.to_owned(),
            crossed: false,
        }
    }

    /// Adds `piece`. False when it must not show: the text crossed the line,
    /// with this piece or before it.
    fn allows(&mut self, piece: &str) -> bool {
        if !self.crossed {
            self.seen.push_str(piece);
            self.crossed = crate::pictures::safety::reply_crosses_line(&self.seen);
        }
        !self.crossed
    }
}

fn empty_reply(raw: &str) -> Error {
    if raw.trim().is_empty() {
        Error::Engine("the reply was empty".into())
    } else {
        Error::Engine(
            "the model wrote only lines for you, and the app removed them. \
             Send your message again, or change it"
                .into(),
        )
    }
}

/// What "Rebuild summary" wrote, and the message that it covers up to.
#[derive(Debug, Clone, Serialize)]
pub struct RebuiltSummary {
    pub summary: String,
    pub through_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewMessage {
    #[serde(flatten)]
    pub message: Message,
    /// IDs of this message and its siblings (other versions under the same parent), oldest first.
    pub siblings: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatView {
    pub chat: Chat,
    pub character: Character,
    pub persona: Option<Persona>,
    pub messages: Vec<ViewMessage>,
    /// Hidden branches that continue from the head, for example after a rewind.
    pub branches: Vec<i64>,
    /// The model that writes the next reply.
    pub model: String,
    /// The chat's own model, when it was removed. The default writes instead.
    pub model_removed: Option<String>,
    /// Ask the persona's age once: see `content::needs_persona_age`.
    pub ask_persona_age: bool,
}

/// Progress of one reply.
pub enum Progress<'a> {
    Text(&'a str),
    /// The guard rejected the text so far and the reply starts again.
    Restart,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplyResult {
    /// `None` when the user stopped the reply before any text came.
    pub message: Option<Message>,
    pub hint: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegenMode {
    Normal,
    Shorter,
    Longer,
    InCharacter,
    NotInCharacter,
}

/// What a deletion needs for its undo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteUndo {
    pub message_id: i64,
    pub stamp: i64,
    pub old_head: Option<i64>,
}

pub struct ChatService {
    db: Db,
    engine: Arc<Engine>,
    // One reply at a time. Holds the token that stops it.
    current: Mutex<Option<CancellationToken>>,
    // Open-thread answers by the id of the user message they came before. A
    // regenerate reuses one, and the next check reads it as the earlier note.
    threads: Mutex<HashMap<i64, Option<String>>>,
    // The running memory pass. A new reply stops it, because the engine
    // writes one answer at a time and a reply must not wait.
    memory_job: Mutex<Option<CancellationToken>>,
    // Set when the app closes: no new memory pass starts after that.
    closing: std::sync::atomic::AtomicBool,
    // The id of the last chat whose memory a pass changed, for the panel.
    memory_changed: watch::Sender<i64>,
}

/// What the picture step gets from the chat.
pub struct PictureScene {
    pub scene: pictures::scene::Scene,
    pub character: Character,
    pub persona: Option<Persona>,
    /// The chat model, to unload before the picture and load again after.
    pub chat_model: String,
    /// The message that the picture belongs to.
    pub message_id: Option<i64>,
    /// The messages read for the scene, or the wish, mention someone under 18.
    pub minor_mentioned: bool,
}

/// Clears the "reply in progress" slot when the reply or picture ends in any way.
pub struct ReplyGuard<'a>(&'a Mutex<Option<CancellationToken>>);

impl Drop for ReplyGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

/// How one reply is written.
#[derive(Default)]
struct ReplyPlan {
    style: Option<StyleSettings>,
    push_character: bool,
}

impl ChatService {
    pub fn new(db: Db, engine: Arc<Engine>) -> Self {
        Self {
            db,
            engine,
            current: Mutex::new(None),
            threads: Mutex::new(HashMap::new()),
            memory_job: Mutex::new(None),
            closing: std::sync::atomic::AtomicBool::new(false),
            memory_changed: watch::Sender::new(0),
        }
    }

    /// Sends the chat id each time a memory pass changes what a chat remembers.
    pub fn subscribe_memory(&self) -> watch::Receiver<i64> {
        self.memory_changed.subscribe()
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Stops the reply in progress. The text so far is kept.
    pub fn stop(&self) {
        if let Ok(slot) = self.current.lock()
            && let Some(token) = slot.as_ref()
        {
            token.cancel();
        }
    }

    /// The app closes: stops the memory pass and starts no new one, so the
    /// engine can stop without a pass failing against it.
    pub fn shut_down(&self) {
        let job = self.memory_job.lock().ok().and_then(|mut j| {
            self.closing
                .store(true, std::sync::atomic::Ordering::SeqCst);
            j.take()
        });
        if let Some(job) = job {
            job.cancel();
        }
    }

    fn begin(&self) -> Result<CancellationToken> {
        let mut slot = self.current.lock().map_err(|_| Error::Busy)?;
        if slot.is_some() {
            return Err(Error::Busy);
        }
        self.stop_memory();
        let token = CancellationToken::new();
        *slot = Some(token.clone());
        Ok(token)
    }

    /// Stops the running memory pass. The next reply starts it again.
    fn stop_memory(&self) {
        if let Some(job) = self.memory_job.lock().ok().and_then(|mut j| j.take()) {
            job.cancel();
        }
    }

    /// Takes the one GPU slot for a picture, like a reply does: a reply and a
    /// picture never run at the same time, and Stop ends either one.
    pub fn begin_job(&self) -> Result<(CancellationToken, ReplyGuard<'_>)> {
        let token = self.begin()?;
        Ok((token, ReplyGuard(&self.current)))
    }

    /// Tree changes wait until no reply is being written.
    /// Fails with `Busy` while a reply or a picture runs.
    pub fn idle(&self) -> Result<()> {
        match self.current.lock() {
            Ok(slot) if slot.is_none() => Ok(()),
            _ => Err(Error::Busy),
        }
    }

    fn model_for(&self, chat: &Chat, character: &Character) -> String {
        self.resolve_model(
            chat.chat_model.as_deref(),
            character.model_override.as_deref(),
        )
    }

    /// The chat's model, else the character's, else the default. A model that
    /// is no longer installed is skipped.
    fn resolve_model(&self, chat_model: Option<&str>, character_model: Option<&str>) -> String {
        [chat_model, character_model]
            .into_iter()
            .flatten()
            .find(|m| self.engine.is_installed(m))
            .map_or_else(|| self.engine.model(), str::to_owned)
    }
}
