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

    // ---- Views -------------------------------------------------------------

    pub async fn view(&self, chat_id: i64) -> Result<ChatView> {
        let chat = self.db.chat(chat_id).await?;
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(&chat).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let mut children: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
        for (id, parent) in self.db.tree_links(chat_id).await? {
            children.entry(parent).or_default().push(id);
        }
        let branches = chat
            .head_message_id
            .and_then(|head| children.get(&Some(head)).cloned())
            .unwrap_or_default();
        let messages = path
            .into_iter()
            .map(|message| ViewMessage {
                siblings: children
                    .get(&message.parent_id)
                    .cloned()
                    .unwrap_or_default(),
                message,
            })
            .collect();
        let model = self.model_for(&chat, &character);
        // The installed list is only known when the engine runs.
        let known = matches!(
            self.engine.status(),
            crate::engine::EngineStatus::Ready { .. }
        );
        let model_removed = chat
            .chat_model
            .clone()
            .filter(|m| known && !self.engine.is_installed(m));
        let ask_persona_age = crate::content::needs_persona_age(
            crate::content::adult_on(&self.db).await?,
            &character,
            persona.as_ref(),
        );
        Ok(ChatView {
            chat,
            character,
            persona,
            messages,
            branches,
            model,
            model_removed,
            ask_persona_age,
        })
    }

    /// The model for this chat's next replies. `None` goes back to the
    /// character's model or the default.
    pub async fn set_model(&self, chat_id: i64, model: Option<&str>) -> Result<ChatView> {
        self.idle()?;
        if let Some(m) = model
            && !self.engine.is_installed(m)
        {
            return Err(Error::Invalid(format!(
                "{m} is not installed. Download it on the Models screen"
            )));
        }
        self.db.set_chat_model(chat_id, model).await?;
        self.view(chat_id).await
    }

    /// The model for a character's new chats and test replies. `None` uses the default.
    pub async fn set_character_model(&self, character_id: i64, model: Option<&str>) -> Result<()> {
        if let Some(m) = model
            && !self.engine.is_installed(m)
        {
            return Err(Error::Invalid(format!(
                "{m} is not installed. Download it on the Models screen"
            )));
        }
        self.db.set_character_model(character_id, model).await
    }

    /// The names of the personas that wrote earlier messages of `history`,
    /// other than `now`, the user's name in the chat now.
    async fn old_user_names(&self, history: &[Message], now: &str) -> Vec<String> {
        let mut ids: Vec<i64> = history
            .iter()
            .filter(|m| m.role == "user")
            .filter_map(|m| m.persona_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let mut names = Vec::new();
        for id in ids {
            // A deleted persona has no name to keep away from.
            if let Ok(p) = self.db.persona(id).await
                && p.name != now
                && !names.contains(&p.name)
            {
                names.push(p.name);
            }
        }
        names
    }

    async fn persona_of(&self, chat: &Chat) -> Result<Option<Persona>> {
        match chat.persona_id {
            Some(id) => match self.db.persona(id).await {
                Ok(p) => Ok(Some(p)),
                Err(Error::NotFound(_)) => Ok(None),
                Err(e) => Err(e),
            },
            None => Ok(None),
        }
    }

    /// The latest chat with a character, or a new one.
    pub async fn open_latest(&self, character_id: i64) -> Result<ChatView> {
        match self.db.latest_chat(character_id).await? {
            Some(id) => self.view(id).await,
            None => {
                let persona = self.db.default_persona().await?;
                self.new_chat(character_id, persona).await
            }
        }
    }

    pub async fn new_chat(&self, character_id: i64, persona_id: Option<i64>) -> Result<ChatView> {
        self.new_chat_using(character_id, persona_id, false).await
    }

    /// `use_memory` is the user's answer when the character's memory setting
    /// is "ask". "off" and "always" decide by themselves.
    pub async fn new_chat_using(
        &self,
        character_id: i64,
        persona_id: Option<i64>,
        use_memory: bool,
    ) -> Result<ChatView> {
        let character = self.db.character(character_id).await?;
        if let Some(id) = persona_id {
            self.db.persona(id).await?;
        }
        let use_memory = match self.db.kept(character_id).await?.mode.as_str() {
            "always" => true,
            "off" => false,
            _ => use_memory,
        };
        let id = self
            .db
            .create_chat(&character, persona_id, "", true)
            .await?;
        if use_memory {
            self.db.set_chat_use_memory(id, true).await?;
        }
        self.view(id).await
    }

    /// What the prompt remembers on `path`: the chat's notes, and the
    /// character's kept facts when the chat uses them.
    async fn recall(
        &self,
        chat: &Chat,
        memories: &[Memory],
        path: &[Message],
    ) -> Result<memory::Recall> {
        let mut recall = memory::recall(memories, path);
        if chat.use_character_memory {
            recall.kept = self
                .db
                .kept(chat.character_id)
                .await?
                .memories
                .into_iter()
                .map(|m| m.content)
                .collect();
        }
        Ok(recall)
    }

    /// "Refresh context": from the next reply on, the prompt keeps only the
    /// last lines up to the newest user message, with the summary and the
    /// memories (`memory::window`). The messages stay. The point is a user
    /// message, so a regenerate of the last reply keeps it on the path.
    pub async fn refresh_context(&self, chat_id: i64) -> Result<ChatView> {
        self.idle()?;
        self.db.chat(chat_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let point = path
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .ok_or_else(|| Error::Invalid("write a message first".into()))?;
        self.db.set_context_start(chat_id, Some(point.id)).await?;
        self.view(chat_id).await
    }

    pub async fn undo_refresh(&self, chat_id: i64) -> Result<ChatView> {
        self.idle()?;
        self.db.chat(chat_id).await?;
        self.db.set_context_start(chat_id, None).await?;
        self.view(chat_id).await
    }

    /// "Next chapter": a new chat with the same character, persona and
    /// settings, linked to this one. It starts with a note that holds the
    /// story so far, the goal and the facts, and the memory pass goes on
    /// from there. The old chat stays.
    pub async fn next_chapter(&self, chat_id: i64) -> Result<ChatView> {
        let old = self.db.chat(chat_id).await?;
        let character = self.db.character(old.character_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let memories = self.db.memories(chat_id).await?;
        let notes: Vec<Memory> = memory::on_path(&memories, &path)
            .into_iter()
            .filter(|m| !m.is_hidden && !m.content.is_empty())
            .collect();
        let chapter = self.db.chapter_of(chat_id).await? + 1;
        let title = format!("Chapter {chapter}");
        let id = self
            .db
            .create_chat(&character, old.persona_id, &title, false)
            .await?;
        self.db.continue_settings(id, &old).await?;
        let text = format!("{title} begins. The story goes on from the last chapter.");
        let note = self
            .db
            .append_message(&NewMessage {
                chat_id: id,
                parent_id: None,
                role: "system",
                persona_id: old.persona_id,
                content: &text,
                ..Default::default()
            })
            .await?;
        for m in notes {
            let copy = self
                .db
                .add_memory(id, &m.kind, &m.content, note.id, &m.origin)
                .await?;
            if m.is_pinned {
                self.db.set_memory_flags(id, copy.id, true, false).await?;
            }
        }
        self.view(id).await
    }

    /// Keeps a fact of this chat for new chats with the character.
    pub async fn keep_memory(&self, chat_id: i64, memory_id: i64) -> Result<crate::db::Kept> {
        let chat = self.db.chat(chat_id).await?;
        let memory = self
            .db
            .memories(chat_id)
            .await?
            .into_iter()
            .find(|m| m.id == memory_id && m.kind == "fact")
            .ok_or(Error::NotFound("memory"))?;
        self.db
            .keep_memory(chat.character_id, &memory.content)
            .await?;
        self.db.kept(chat.character_id).await
    }

    pub async fn chats(&self, character_id: i64) -> Result<Vec<ChatSummary>> {
        self.db.chat_summaries(character_id).await
    }

    pub async fn delete_chat(&self, chat_id: i64) -> Result<()> {
        self.db.chat(chat_id).await?;
        self.db.delete_chat(chat_id).await
    }

    // ---- Chat settings -----------------------------------------------------

    /// Changes the persona. A hidden note tells the model about the change at this point.
    pub async fn set_persona(&self, chat_id: i64, persona_id: Option<i64>) -> Result<ChatView> {
        let chat = self.db.chat(chat_id).await?;
        let persona = match persona_id {
            Some(id) => Some(self.db.persona(id).await?),
            None => None,
        };
        if chat.persona_id == persona_id {
            return self.view(chat_id).await;
        }
        self.idle()?;
        self.db.set_chat_persona(chat_id, persona_id).await?;
        if chat.head_message_id.is_some() {
            let note = match &persona {
                Some(p) if !p.description.trim().is_empty() => {
                    format!("The user is now {}: {}", p.name, p.description.trim())
                }
                Some(p) => format!("The user is now {}.", p.name),
                None => format!("The user is now {}.", prompt::NO_PERSONA_NAME),
            };
            self.db
                .append_message(&NewMessage {
                    chat_id,
                    parent_id: chat.head_message_id,
                    role: "system",
                    persona_id,
                    content: &note,
                    ..Default::default()
                })
                .await?;
        }
        self.view(chat_id).await
    }

    pub async fn set_tone(&self, chat_id: i64, tone: &str, custom: &str) -> Result<ChatView> {
        if !presets::TONES.contains(&tone) {
            return Err(Error::Invalid(format!("unknown tone {tone}")));
        }
        let custom = clean_line(custom, presets::MAX_CUSTOM_TONE);
        self.db.chat(chat_id).await?;
        self.db.set_chat_tone(chat_id, tone, &custom).await?;
        self.view(chat_id).await
    }

    pub async fn set_style(
        &self,
        chat_id: i64,
        style: &str,
        custom: Option<StyleSettings>,
    ) -> Result<ChatView> {
        if !presets::STYLES.contains(&style) {
            return Err(Error::Invalid(format!("unknown reply style {style}")));
        }
        let custom = match (style, custom) {
            ("custom", Some(settings)) => serde_json::to_string(&settings)?,
            ("custom", None) => return Err(Error::Invalid("custom style needs settings".into())),
            _ => String::new(),
        };
        self.db.chat(chat_id).await?;
        self.db.set_chat_style(chat_id, style, &custom).await?;
        self.view(chat_id).await
    }

    // ---- Tree operations ---------------------------------------------------

    /// Moves the head, for "Rewind to here" and for its undo.
    pub async fn set_head(&self, chat_id: i64, message_id: i64) -> Result<ChatView> {
        self.idle()?;
        self.db.message(chat_id, message_id).await?;
        self.db.set_chat_head(chat_id, Some(message_id)).await?;
        self.sync_persona(chat_id).await?;
        self.view(chat_id).await
    }

    /// Shows another version of a message: the head moves to the newest end of its branch.
    pub async fn show_branch(&self, chat_id: i64, message_id: i64) -> Result<ChatView> {
        self.idle()?;
        self.db.message(chat_id, message_id).await?;
        let mut newest_child: HashMap<i64, i64> = HashMap::new();
        for (id, parent) in self.db.tree_links(chat_id).await? {
            if let Some(parent) = parent {
                // Links come oldest first, so the newest child wins.
                newest_child.insert(parent, id);
            }
        }
        let mut head = message_id;
        while let Some(&next) = newest_child.get(&head) {
            head = next;
        }
        self.db.set_chat_head(chat_id, Some(head)).await?;
        self.sync_persona(chat_id).await?;
        self.view(chat_id).await
    }

    /// Deletes a message and all later messages on its branch. They stay in the
    /// database, so `undo_delete` can bring them back.
    pub async fn delete_message(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<(ChatView, DeleteUndo)> {
        self.idle()?;
        let chat = self.db.chat(chat_id).await?;
        let message = self.db.message(chat_id, message_id).await?;
        let stamp = now_ms();
        self.db.delete_subtree(chat_id, message_id, stamp).await?;
        let head_gone = match chat.head_message_id {
            Some(head) => self.db.message(chat_id, head).await.is_err(),
            None => true,
        };
        if head_gone {
            self.db.set_chat_head(chat_id, message.parent_id).await?;
        }
        self.sync_persona(chat_id).await?;
        let undo = DeleteUndo {
            message_id,
            stamp,
            old_head: chat.head_message_id,
        };
        Ok((self.view(chat_id).await?, undo))
    }

    pub async fn undo_delete(&self, chat_id: i64, undo: &DeleteUndo) -> Result<ChatView> {
        self.idle()?;
        self.db.chat(chat_id).await?;
        self.db
            .restore_subtree(chat_id, undo.message_id, undo.stamp)
            .await?;
        if let Some(head) = undo.old_head
            && self.db.message(chat_id, head).await.is_ok()
        {
            self.db.set_chat_head(chat_id, Some(head)).await?;
        }
        self.sync_persona(chat_id).await?;
        self.view(chat_id).await
    }

    /// Copies the path up to a message into a new chat.
    pub async fn branch(&self, chat_id: i64, message_id: i64) -> Result<ChatView> {
        let chat = self.db.chat(chat_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let end = position(&path, message_id)?;
        let character = self.db.character(chat.character_id).await?;
        let new_id = self
            .db
            .create_chat(&character, chat.persona_id, "", false)
            .await?;
        self.db
            .set_chat_tone(new_id, &chat.tone, &chat.tone_custom)
            .await?;
        self.db
            .set_chat_style(new_id, &chat.reply_style, &chat.reply_custom)
            .await?;
        self.db.copy_path(new_id, &path[..=end]).await?;
        self.view(new_id).await
    }

    /// The user fixes a reply by hand. The text is saved in place.
    pub async fn edit_reply_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> Result<ChatView> {
        self.idle()?;
        let message = self.db.message(chat_id, message_id).await?;
        if message.role != "character" {
            return Err(Error::Invalid("only replies are edited in place".into()));
        }
        let text = clean_input(text)?;
        self.db
            .update_message_content(message_id, &text, None, None)
            .await?;
        self.view(chat_id).await
    }

    /// The persona of the chat follows the head, so a rewind restores it.
    async fn sync_persona(&self, chat_id: i64) -> Result<()> {
        let path = self.db.path_to_head(chat_id).await?;
        if let Some(persona) = path
            .iter()
            .rev()
            .filter(|m| matches!(m.role.as_str(), "user" | "ooc" | "system"))
            .find_map(|m| m.persona_id)
            && self.db.persona(persona).await.is_ok()
        {
            self.db.set_chat_persona(chat_id, Some(persona)).await?;
        }
        Ok(())
    }

    // ---- Replies -----------------------------------------------------------

    /// Saves the user's message, then writes the reply.
    /// `/ooc text` and `((text))` are notes to the model, outside the story.
    pub async fn send(
        &self,
        chat_id: i64,
        text: &str,
        on_user_saved: impl FnOnce(&Message),
        progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let (role, text) = split_ooc(&clean_input(text)?)?;
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        // Check the engine before saving, so a failed send does not leave a lone message.
        self.engine.client()?;
        let chat = self.db.chat(chat_id).await?;
        let user = self
            .db
            .append_message(&NewMessage {
                chat_id,
                parent_id: chat.head_message_id,
                role,
                persona_id: chat.persona_id,
                content: &text,
                ..Default::default()
            })
            .await?;
        on_user_saved(&user);
        let history = self.db.path_to_head(chat_id).await?;
        self.reply(&chat, &history, ReplyPlan::default(), &cancel, progress)
            .await
    }

    /// Edits a user message: the new text becomes a new version, and a new reply follows.
    pub async fn edit_and_resend(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        on_user_saved: impl FnOnce(&Message),
        progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let (role, text) = split_ooc(&clean_input(text)?)?;
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        self.engine.client()?;
        let chat = self.db.chat(chat_id).await?;
        let old = self.db.message(chat_id, message_id).await?;
        if !matches!(old.role.as_str(), "user" | "ooc") {
            return Err(Error::Invalid("this is not a user message".into()));
        }
        let user = self
            .db
            .append_message(&NewMessage {
                chat_id,
                parent_id: old.parent_id,
                role,
                persona_id: old.persona_id.or(chat.persona_id),
                content: &text,
                ..Default::default()
            })
            .await?;
        on_user_saved(&user);
        let history = self.db.path_to_head(chat_id).await?;
        self.reply(&chat, &history, ReplyPlan::default(), &cancel, progress)
            .await
    }

    /// Writes a reply to the head message again, for example after an error.
    pub async fn retry(
        &self,
        chat_id: i64,
        progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        let chat = self.db.chat(chat_id).await?;
        let history = self.db.path_to_head(chat_id).await?;
        self.reply(&chat, &history, ReplyPlan::default(), &cancel, progress)
            .await
    }

    /// Writes a new version of a reply. The old version stays as a sibling.
    pub async fn regenerate(
        &self,
        chat_id: i64,
        message_id: i64,
        mode: RegenMode,
        progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        let chat = self.db.chat(chat_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let at = position(&path, message_id)?;
        if path[at].role != "character" {
            return Err(Error::Invalid("only replies are regenerated".into()));
        }
        if mode == RegenMode::NotInCharacter {
            self.db.flag_out_of_character(message_id).await?;
        }
        let base = StyleSettings::of(&chat.reply_style, &chat.reply_custom);
        let plan = ReplyPlan {
            style: match mode {
                RegenMode::Shorter => Some(base.shifted(false)),
                RegenMode::Longer => Some(base.shifted(true)),
                _ => None,
            },
            push_character: matches!(mode, RegenMode::InCharacter | RegenMode::NotInCharacter),
        };
        // The greeting has no user message before it, so it gets its own path.
        if path[at].parent_id.is_none() {
            return self
                .new_greeting(&chat, &path[at], plan, &cancel, progress)
                .await;
        }
        self.reply(&chat, &path[..at], plan, &cancel, progress)
            .await
    }

    /// Writes a new version of the greeting. The card's greeting stays as the
    /// first version, and new chats always start with it.
    async fn new_greeting(
        &self,
        chat: &Chat,
        greeting: &Message,
        plan: ReplyPlan,
        cancel: &CancellationToken,
        mut progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let client = self.engine.client()?;
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(chat).await?;
        let style = plan
            .style
            .unwrap_or_else(|| StyleSettings::of(&chat.reply_style, &chat.reply_custom));
        let model = self.model_for(chat, &character);
        let tuning = self.engine.tuning_for(&model);
        let request = Message {
            id: 0,
            chat_id: chat.id,
            parent_id: None,
            role: "user".into(),
            persona_id: chat.persona_id,
            content: format!(
                "(Story note, not said by anyone: the chat starts now. Write the opening message of the story as {}.)",
                character.name
            ),
            flagged_out_of_character: false,
            model: None,
            created_at: 0,
        };
        let note = format!(
            "This is the first message of a new chat. The usual opening is below. Keep its setting and situation, but write a new opening in your own words. Do not copy it.\nUsual opening: \"{}\"",
            greeting.content
        );
        let mut extra = note.clone();
        let mut retried = false;
        let (outcome, content, prompt, hint) = loop {
            let mature =
                crate::content::mature_chat(&self.db, &character, persona.as_ref()).await?;
            let prompt = prompt::build(&PromptInput {
                character: &character,
                persona: persona.as_ref(),
                tone: &chat.tone,
                tone_custom: &chat.tone_custom,
                style,
                history: std::slice::from_ref(&request),
                push_character: plan.push_character,
                open_thread: None,
                recall: None,
                extra_reminder: Some(&extra),
                continuation: false,
                tuning: &tuning,
                old_user_names: &[],
                mature,
            });
            let stop = cancel.child_token();
            let mut line = LineWatch::new("");
            let mut outcome = client
                .chat(
                    &chat_params(&model, &prompt.messages, &prompt.options, tuning.think),
                    REPLY_TIMEOUT,
                    &stop,
                    |t| {
                        if line.allows(t) {
                            progress(Progress::Text(t));
                        } else {
                            stop.cancel();
                        }
                    },
                )
                .await?;
            outcome.stopped = cancel.is_cancelled();
            let content = guard::clean(
                &outcome.text,
                &character.name,
                &prompt.user_name,
                outcome.cut_at_cap,
                false,
            );
            refuse_crossing(&outcome.text)?;
            let problem = if outcome.stopped {
                None
            } else if content.is_empty() {
                Some(Problem::CharacterBreak)
            } else {
                // A copy of the old greeting counts as repetition.
                guard::check(&content, &[greeting.content.as_str()])
            };
            match problem {
                Some(p) if !retried => {
                    log::info!("silent regenerate of the greeting: {p:?}");
                    retried = true;
                    progress(Progress::Restart);
                    extra = format!(
                        "{note}
{}",
                        p.reminder().replace("{{char}}", &character.name)
                    );
                }
                Some(Problem::CharacterBreak) => {
                    break (outcome, content, prompt, Some(OUT_OF_CHARACTER_HINT));
                }
                _ => break (outcome, content, prompt, None),
            }
        };
        if content.is_empty() {
            return if outcome.stopped {
                Ok(ReplyResult {
                    message: None,
                    hint: None,
                })
            } else {
                Err(empty_reply(&outcome.text))
            };
        }
        let sampler = serde_json::to_string(&prompt.options)?;
        let saved = self
            .db
            .append_message(&NewMessage {
                chat_id: chat.id,
                parent_id: None,
                role: "character",
                content: &content,
                raw_content: Some(&outcome.text),
                token_count: outcome.eval_count,
                model: Some(&model),
                prompt_version: Some(PROMPT_VERSION),
                sampler_json: Some(&sampler),
                ..Default::default()
            })
            .await?;
        Ok(ReplyResult {
            message: Some(saved),
            hint,
        })
    }

    /// Makes the last reply longer. The new text is added to the same message.
    pub async fn continue_reply(
        &self,
        chat_id: i64,
        mut progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        let client = self.engine.client()?;
        let chat = self.db.chat(chat_id).await?;
        let history = self.db.path_to_head(chat_id).await?;
        let Some(last) = history.last().filter(|m| m.role == "character") else {
            return Err(Error::Invalid("the last message is not a reply".into()));
        };
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(&chat).await?;
        // A reply cut in the middle of a sentence is continued directly. A
        // finished reply gets one new paragraph, because the model stops at once
        // when it continues a finished text.
        let mid_sentence = !ends_sentence(&last.content);
        let now = persona
            .as_ref()
            .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str());
        let old_names = self.old_user_names(&history, now).await;
        let model = self.model_for(&chat, &character);
        let tuning = self.engine.tuning_for(&model);
        let memories = self.db.memories(chat.id).await?;
        let recall = self.recall(&chat, &memories, &history).await?;
        let mature = crate::content::mature_chat(&self.db, &character, persona.as_ref()).await?;
        let prompt = prompt::build(&PromptInput {
            character: &character,
            persona: persona.as_ref(),
            tone: &chat.tone,
            tone_custom: &chat.tone_custom,
            style: StyleSettings::of(&chat.reply_style, &chat.reply_custom),
            history: memory::window(&history, chat.context_start_id, &memories),
            push_character: false,
            open_thread: None,
            recall: Some(&recall),
            extra_reminder: Some(CONTINUE_REMINDER),
            continuation: mid_sentence,
            tuning: &tuning,
            old_user_names: &old_names,
            mature,
        });
        let stop = cancel.child_token();
        let mut line = LineWatch::new(&last.content);
        let outcome = client
            .chat(
                &chat_params(&model, &prompt.messages, &prompt.options, tuning.think),
                REPLY_TIMEOUT,
                &stop,
                |t| {
                    if line.allows(t) {
                        progress(Progress::Text(t));
                    } else {
                        stop.cancel();
                    }
                },
            )
            .await?;
        let joined = join_continuation(&last.content, &outcome.text);
        refuse_crossing(&joined)?;
        let content = guard::clean(
            &joined,
            &character.name,
            &prompt.user_name,
            outcome.cut_at_cap,
            true,
        );
        self.db
            .update_message_content(last.id, &content, Some(&joined), None)
            .await?;
        let message = self.db.message(chat_id, last.id).await?;
        Ok(ReplyResult {
            message: Some(message),
            hint: None,
        })
    }

    /// Stops the reply in progress. The text so far is kept.
    pub fn stop(&self) {
        if let Ok(slot) = self.current.lock()
            && let Some(token) = slot.as_ref()
        {
            token.cancel();
        }
    }

    /// A reply for the creator's "Test chat". Nothing is saved.
    pub async fn test_reply(
        &self,
        character: &Character,
        persona: Option<&Persona>,
        history: &[Message],
        mut progress: impl FnMut(Progress<'_>),
    ) -> Result<String> {
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        let client = self.engine.client()?;
        let model = self.resolve_model(None, character.model_override.as_deref());
        let tuning = self.engine.tuning_for(&model);
        let mature = crate::content::mature_chat(&self.db, character, persona).await?;
        let prompt = prompt::build(&PromptInput {
            character,
            persona,
            tone: &character.default_tone,
            tone_custom: "",
            style: StyleSettings::of(
                &character.default_reply_style,
                &character.default_reply_custom,
            ),
            history,
            push_character: false,
            open_thread: None,
            recall: None,
            extra_reminder: None,
            continuation: false,
            tuning: &tuning,
            old_user_names: &[],
            mature,
        });
        let stop = cancel.child_token();
        let mut line = LineWatch::new("");
        let outcome = client
            .chat(
                &chat_params(&model, &prompt.messages, &prompt.options, tuning.think),
                REPLY_TIMEOUT,
                &stop,
                |t| {
                    if line.allows(t) {
                        progress(Progress::Text(t));
                    } else {
                        stop.cancel();
                    }
                },
            )
            .await?;
        refuse_crossing(&outcome.text)?;
        Ok(guard::clean(
            &outcome.text,
            &character.name,
            &prompt.user_name,
            outcome.cut_at_cap,
            false,
        ))
    }

    /// Suggests a voice anchor (2 to 4 short sentences) from the personality.
    pub async fn suggest_anchor(&self, name: &str, personality: &str) -> Result<String> {
        let client = self.engine.client()?;
        let name = clean_line(name, 80);
        let personality: String = personality.chars().take(4000).collect();
        if personality.trim().is_empty() {
            return Err(Error::Invalid("write the personality first".into()));
        }
        let messages = [
            ChatMessage {
                role: "system",
                images: Vec::new(),
                content: "You help write roleplay characters. Answer with plain text only.".into(),
            },
            ChatMessage {
                role: "user",
                images: Vec::new(),
                content: format!(
                    "Character: {name}\nPersonality: {personality}\n\n\
                     Write the voice of this character in 2 to 4 short sentences: how they talk, \
                     their top traits and their speech habits. Example: \"Short, dry sentences. \
                     Sarcastic, tired, honest. Calls people 'kid'. Never uses exclamation marks.\" \
                     Write only the sentences."
                ),
            },
        ];
        let options = Options {
            temperature: 0.5,
            min_p: 0.05,
            top_p: 1.0,
            top_k: 40,
            presence_penalty: 0.0,
            repeat_penalty: 1.08,
            repeat_last_n: 64,
            num_ctx: 2048,
            num_predict: 120,
            stop: Vec::new(),
        };
        let model = self.engine.model();
        let think = self.engine.tuning_for(&model).think.map(|_| false);
        let text = client
            .complete(
                &chat_params(&model, &messages, &options, think),
                REPLY_TIMEOUT,
            )
            .await?;
        Ok(clean_line(text.trim().trim_matches('"'), 400))
    }

    /// True when the chosen chat model can read pictures.
    pub async fn can_see(&self) -> bool {
        let Ok(client) = self.engine.client() else {
            return false;
        };
        client
            .info(&self.engine.model())
            .await
            .is_ok_and(|i| i.can_see())
    }

    /// Looks and an outfit read from a profile picture (`picture` from
    /// `avatar::for_vision`). The user checks them before saving.
    pub async fn suggest_looks_from_picture(
        &self,
        name: &str,
        picture: String,
    ) -> Result<pictures::looks::Suggested> {
        // Holds the GPU slot, so no picture starts while the model reads.
        let (token, _guard) = self.begin_job()?;
        if !self.can_see().await {
            return Err(Error::Invalid(
                "the chosen chat model cannot read pictures".into(),
            ));
        }
        let client = self.engine.client()?;
        let name = clean_line(name, 80);
        let messages = [ChatMessage {
            role: "user",
            images: vec![picture],
            content: pictures::looks::picture_prompt(&name),
        }];
        let options = Options {
            temperature: 0.2,
            min_p: 0.05,
            top_p: 1.0,
            top_k: 40,
            presence_penalty: 0.0,
            repeat_penalty: 1.08,
            repeat_last_n: 64,
            num_ctx: 4096,
            num_predict: 160,
            stop: Vec::new(),
        };
        let model = self.engine.model();
        let think = self.engine.tuning_for(&model).think.map(|_| false);
        let params = chat_params(&model, &messages, &options, think);
        // Stop ends it, so a closed creator does not hold the slot.
        let text = tokio::select! {
            biased;
            () = token.cancelled() => return Err(Error::Cancelled),
            text = client.complete(&params, REPLY_TIMEOUT) => text?,
        };
        Ok(pictures::looks::parse(&text))
    }

    /// Suggests Looks and a usual outfit from a card's text, for a card that
    /// has none. Nothing is saved: the creator shows the text to check first.
    pub async fn suggest_looks(
        &self,
        name: &str,
        card: &str,
    ) -> Result<pictures::looks::Suggested> {
        // Holds the GPU slot, so no picture starts while the model writes.
        let (token, _guard) = self.begin_job()?;
        let client = self.engine.client()?;
        let name = clean_line(name, 80);
        let card: String = card.chars().take(4000).collect();
        if card.trim().is_empty() {
            return Err(Error::Invalid(
                "write the personality or greeting first".into(),
            ));
        }
        let messages = [
            ChatMessage {
                role: "system",
                images: Vec::new(),
                content: "You help write roleplay characters. Answer with plain text only.".into(),
            },
            ChatMessage {
                role: "user",
                images: Vec::new(),
                content: pictures::looks::prompt(&name, &card),
            },
        ];
        let options = Options {
            temperature: 0.4,
            min_p: 0.05,
            top_p: 1.0,
            top_k: 40,
            presence_penalty: 0.0,
            repeat_penalty: 1.08,
            repeat_last_n: 64,
            num_ctx: 4096,
            num_predict: 160,
            stop: Vec::new(),
        };
        let model = self.engine.model();
        let think = self.engine.tuning_for(&model).think.map(|_| false);
        let params = chat_params(&model, &messages, &options, think);
        // Stop ends it, so a closed creator does not hold the slot.
        let text = tokio::select! {
            biased;
            () = token.cancelled() => return Err(Error::Cancelled),
            text = client.complete(&params, REPLY_TIMEOUT) => text?,
        };
        Ok(pictures::looks::parse(&text))
    }

    /// What the chat remembers on its current path, for the memory panel.
    pub async fn memories(&self, chat_id: i64) -> Result<Vec<Memory>> {
        let path = self.db.path_to_head(chat_id).await?;
        let all = self.db.memories(chat_id).await?;
        Ok(memory::on_path(&all, &path))
    }

    /// Saves a fact that the user wrote, from a message on the current path.
    /// "Remember this" on a message and the panel's own field both use it.
    pub async fn remember(&self, chat_id: i64, message_id: i64, text: &str) -> Result<Vec<Memory>> {
        let note =
            memory::user_note(text, "fact").ok_or_else(|| Error::Invalid("empty note".into()))?;
        let path = self.db.path_to_head(chat_id).await?;
        if !path.iter().any(|m| m.id == message_id) {
            return Err(Error::NotFound("message"));
        }
        self.stop_memory();
        self.db
            .add_memory(chat_id, "fact", &note, message_id, "user")
            .await?;
        self.memories(chat_id).await
    }

    /// Changes the text of a memory. The running pass stops, so that it
    /// does not write a summary from the old text over the edit.
    pub async fn edit_memory(&self, chat_id: i64, id: i64, text: &str) -> Result<Vec<Memory>> {
        let old = self.db.memory(chat_id, id).await?;
        let note = memory::user_note(text, &old.kind)
            .ok_or_else(|| Error::Invalid("empty note".into()))?;
        self.stop_memory();
        self.db.edit_memory(chat_id, id, &note).await?;
        self.memories(chat_id).await
    }

    /// Pins or hides a memory. Only a fact can be pinned.
    pub async fn set_memory_flags(
        &self,
        chat_id: i64,
        id: i64,
        pinned: bool,
        hidden: bool,
    ) -> Result<Vec<Memory>> {
        let old = self.db.memory(chat_id, id).await?;
        self.stop_memory();
        self.db
            .set_memory_flags(chat_id, id, pinned && old.kind == "fact", hidden)
            .await?;
        self.memories(chat_id).await
    }

    /// Updates what the chat remembers: the story so far and the facts that
    /// the user stated, from the messages that are about to leave the
    /// prompt. The app runs it after each reply. A new reply stops it, and
    /// the next pass goes on from where this one stopped.
    pub async fn update_memory(&self, chat_id: i64) -> Result<()> {
        let token = CancellationToken::new();
        {
            let Ok(mut job) = self.memory_job.lock() else {
                return Ok(());
            };
            // Checked under the lock that `shut_down` takes, so no pass
            // starts after it.
            if self.closing.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(());
            }
            if let Some(old) = job.replace(token.clone()) {
                old.cancel();
            }
        }
        match self.memory_passes(chat_id, &token).await {
            // A stopped pass can fail on its way out (the engine closes):
            // that is a stop, not an error.
            Err(_) if token.is_cancelled() => Ok(()),
            result => result,
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

    async fn memory_passes(&self, chat_id: i64, token: &CancellationToken) -> Result<()> {
        while !token.is_cancelled() && self.idle().is_ok() {
            let chat = self.db.chat(chat_id).await?;
            let path = self.db.path_to_head(chat_id).await?;
            let memories = self.db.memories(chat_id).await?;
            let Some(chunk) = memory::pending(&memories, &path) else {
                break;
            };
            let Some(last) = chunk.last() else { break };
            let character = self.db.character(chat.character_id).await?;
            let persona = self.persona_of(&chat).await?;
            let user_name = persona
                .as_ref()
                .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str());
            let old = memory::recall(&memories, &path);
            let question = memory::pass_question(&character.name, user_name, &old, chunk);
            let model = self.model_for(&chat, &character);
            let tuning = self.engine.tuning_for(&model);
            let started = std::time::Instant::now();
            let Some(answer) = self.ask_pass(&model, &tuning, question, token).await? else {
                break;
            };
            let user_messages: Vec<&Message> = chunk.iter().filter(|m| m.role == "user").collect();
            let user_lines: Vec<&str> = user_messages.iter().map(|m| m.content.as_str()).collect();
            let parsed = memory::parse_pass(&answer, &user_lines, &[&character.name, user_name]);
            // The summary marks the chunk as read. With no new summary, the
            // old one carries on, so the next pass does not read it again.
            let summary = parsed.summary.or(old.summary).unwrap_or_default();
            self.db
                .add_memory(chat_id, "summary", &summary, last.id, "auto")
                .await?;
            // "none" clears the goal: an empty goal on the path hides the old one.
            let goal = parsed.goal.unwrap_or_default();
            self.db
                .add_memory(chat_id, "goal", &goal, last.id, "auto")
                .await?;
            log::debug!("memory note: goal: {goal}");
            // Chat text, so debug only: the app log stays at info.
            log::debug!("memory note: summary: {summary}");
            let mut known: Vec<String> = memories
                .iter()
                .filter(|m| m.kind == "fact")
                .map(|m| m.content.clone())
                .collect();
            let mut added = 0;
            for fact in parsed.facts {
                if memory::is_duplicate(&fact, &known) {
                    continue;
                }
                let source = memory::source_of(&fact, &user_messages).unwrap_or(last);
                self.db
                    .add_memory(chat_id, "fact", &fact, source.id, "auto")
                    .await?;
                log::debug!("memory note: fact: {fact}");
                known.push(fact);
                added += 1;
            }
            self.memory_changed.send_replace(chat_id);
            log::info!(
                "memory pass in {} ms: {} messages, summary {} words, {added} new facts",
                started.elapsed().as_millis(),
                chunk.len(),
                summary.split_whitespace().count()
            );
        }
        Ok(())
    }

    /// One memory pass question to the model. `None` when `token` stops it.
    async fn ask_pass(
        &self,
        model: &str,
        tuning: &crate::catalog::Tuning,
        question: String,
        token: &CancellationToken,
    ) -> Result<Option<String>> {
        let client = self.engine.client()?;
        let messages = [
            ChatMessage {
                role: "system",
                images: Vec::new(),
                content: "You keep notes about a roleplay chat. Answer with plain text only."
                    .into(),
            },
            ChatMessage {
                role: "user",
                images: Vec::new(),
                content: question,
            },
        ];
        let options = Options {
            temperature: 0.0,
            min_p: 0.0,
            top_p: 1.0,
            top_k: 1,
            presence_penalty: 0.0,
            repeat_penalty: 1.0,
            repeat_last_n: 64,
            // The chat's own size: another size makes the engine load the model again.
            num_ctx: tuning.context,
            num_predict: MEMORY_TOKENS,
            stop: Vec::new(),
        };
        let params = chat_params(model, &messages, &options, tuning.think.map(|_| false));
        tokio::select! {
            biased;
            () = token.cancelled() => Ok(None),
            a = client.complete(&params, MEMORY_TIMEOUT) => Ok(Some(a?)),
        }
    }

    /// "Rebuild summary": writes the story so far again from the start of
    /// the chat, in the same chunks as the memory pass, but from an empty
    /// summary. It saves nothing: the user reads it first (`save_summary`).
    /// It holds the GPU slot, so Stop ends it and no reply runs meanwhile.
    pub async fn rebuild_summary(&self, chat_id: i64) -> Result<RebuiltSummary> {
        let chat = self.db.chat(chat_id).await?;
        let (token, _guard) = self.begin_job()?;
        self.stop_memory();
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(&chat).await?;
        let user_name = persona
            .as_ref()
            .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str());
        let model = self.model_for(&chat, &character);
        let tuning = self.engine.tuning_for(&model);
        let path = self.db.path_to_head(chat_id).await?;
        // A stand-in summary, never saved, marks how far the passes read.
        let mut read: Option<Memory> = None;
        while let Some(chunk) = memory::pending(read.as_slice(), &path) {
            let Some(last) = chunk.last() else { break };
            let old = memory::recall(read.as_slice(), &path);
            let question = memory::pass_question(&character.name, user_name, &old, chunk);
            let Some(answer) = self.ask_pass(&model, &tuning, question, &token).await? else {
                return Err(Error::Cancelled);
            };
            let user_lines: Vec<&str> = chunk
                .iter()
                .filter(|m| m.role == "user")
                .map(|m| m.content.as_str())
                .collect();
            let parsed = memory::parse_pass(&answer, &user_lines, &[&character.name, user_name]);
            read = Some(Memory {
                id: 0,
                chat_id,
                kind: "summary".into(),
                content: parsed.summary.or(old.summary).unwrap_or_default(),
                source_message_id: last.id,
                origin: "auto".into(),
                is_pinned: false,
                is_hidden: false,
                created_at: 0,
            });
        }
        match read {
            None => Err(Error::Invalid("the chat is too short for a summary".into())),
            Some(m) if m.content.is_empty() => Err(Error::Invalid(
                "the model wrote no summary, try again".into(),
            )),
            Some(m) => Ok(RebuiltSummary {
                summary: m.content,
                through_id: m.source_message_id,
            }),
        }
    }

    /// Saves a rebuilt summary, as the user's own, on the message it covers
    /// up to. The memory pass goes on after it. If the chat moved on so that
    /// the message left the path or a newer summary exists, the rebuild is
    /// out of date and nothing is saved.
    pub async fn save_summary(
        &self,
        chat_id: i64,
        through_id: i64,
        text: &str,
    ) -> Result<Vec<Memory>> {
        let note = memory::user_note(text, "summary")
            .ok_or_else(|| Error::Invalid("empty summary".into()))?;
        self.db.chat(chat_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        let stale = || Error::Invalid("the chat changed, rebuild the summary again".into());
        let at = path
            .iter()
            .position(|m| m.id == through_id)
            .ok_or_else(stale)?;
        let newer = self
            .db
            .memories(chat_id)
            .await?
            .iter()
            .filter(|m| m.kind == "summary")
            .filter_map(|m| path.iter().position(|p| p.id == m.source_message_id))
            .any(|p| p > at);
        if newer {
            return Err(stale());
        }
        self.stop_memory();
        self.db
            .add_memory(chat_id, "summary", &note, through_id, "user")
            .await?;
        self.memories(chat_id).await
    }

    /// Asks the model what the user still waits for from earlier in the chat.
    /// A failure only costs the hint, so it is logged and skipped.
    async fn open_thread(
        &self,
        client: &crate::ollama::Client,
        model: &str,
        tuning: &crate::catalog::Tuning,
        char_name: &str,
        user_name: &str,
        history: &[Message],
    ) -> Option<String> {
        let newest_message = history.last()?;
        let newest = newest_message.id;
        let cached = |id: i64| self.threads.lock().ok().and_then(|t| t.get(&id).cloned());
        if let Some(hit) = cached(newest) {
            return hit;
        }
        // A full message means the user leads, so the character follows it.
        // Storing "nothing" also ends the chain of notes, so a wrong note
        // from the model cannot come back again and again.
        if newest_message.content.split_whitespace().count() > THREAD_SHORT_WORDS {
            if let Ok(mut t) = self.threads.lock() {
                t.insert(newest, None);
            }
            return None;
        }
        let previous = history
            .iter()
            .rev()
            .filter(|m| m.role == "user")
            .nth(1)
            .and_then(|m| cached(m.id))
            .flatten();
        let question =
            prompt::open_thread_question(char_name, user_name, history, previous.as_deref())?;
        let messages = [
            ChatMessage {
                role: "system",
                images: Vec::new(),
                content: "You read a roleplay chat and answer one question about it. Answer with plain text only."
                    .into(),
            },
            ChatMessage {
                role: "user",
                images: Vec::new(),
                content: question,
            },
        ];
        let options = Options {
            temperature: 0.0,
            min_p: 0.0,
            top_p: 1.0,
            top_k: 1,
            presence_penalty: 0.0,
            repeat_penalty: 1.0,
            repeat_last_n: 64,
            // The chat's own size: another size makes the engine load the model again.
            num_ctx: tuning.context,
            num_predict: 40,
            stop: Vec::new(),
        };
        let started = std::time::Instant::now();
        let answer = client
            .complete(
                &chat_params(model, &messages, &options, tuning.think.map(|_| false)),
                THREAD_TIMEOUT,
            )
            .await;
        match answer {
            Ok(text) => {
                let thread = prompt::clean_open_thread(&text);
                log::info!(
                    "open thread in {} ms: {thread:?}",
                    started.elapsed().as_millis()
                );
                if let Ok(mut t) = self.threads.lock() {
                    if t.len() > THREAD_CACHE {
                        t.clear();
                    }
                    t.insert(newest, thread.clone());
                }
                thread
            }
            Err(e) => {
                log::warn!("open thread skipped: {e}");
                None
            }
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

    /// The scene of a picture, from the chat model. `message_id` is the
    /// message to picture, or the latest one when `None`. Falls back to a
    /// plain scene when the model gives no usable answer twice.
    pub async fn picture_scene(
        &self,
        chat_id: i64,
        message_id: Option<i64>,
        wish: &str,
        token: &CancellationToken,
    ) -> Result<PictureScene> {
        let (character, persona, model) = self.picture_people(chat_id).await?;
        let mut path = self.db.path_to_head(chat_id).await?;
        if let Some(id) = message_id
            && let Some(at) = path.iter().position(|m| m.id == id)
        {
            path.truncate(at + 1);
        }
        let user_name = persona
            .as_ref()
            .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str())
            .to_owned();
        let recent: Vec<&Message> = path
            .iter()
            .filter(|m| m.role == "user" || m.role == "character")
            .collect();
        let recent = &recent[recent
            .len()
            .saturating_sub(pictures::scene::RECENT_MESSAGES)..];
        let lines: Vec<(String, String)> = recent
            .iter()
            .map(|m| {
                let name = if m.role == "user" {
                    user_name.clone()
                } else {
                    character.name.clone()
                };
                (name, m.content.clone())
            })
            .collect();
        let last = recent.last().map_or("", |m| m.content.as_str());
        let minor_mentioned = pictures::safety::mentions_minor(wish)
            || crate::content::profile_minor_word(&character, persona.as_ref()).is_some()
            || recent
                .iter()
                .any(|m| pictures::safety::mentions_minor(&m.content));
        let tuning = self.engine.tuning_for(&model);
        let client = self.engine.client()?;
        let schema = pictures::scene::schema();
        let messages = [
            ChatMessage {
                role: "system",
                images: Vec::new(),
                content: pictures::scene::SYSTEM.into(),
            },
            ChatMessage {
                role: "user",
                images: Vec::new(),
                content: pictures::scene::question(&character.name, &user_name, &lines, wish),
            },
        ];
        let options = Options {
            temperature: 0.2,
            min_p: 0.0,
            top_p: 1.0,
            top_k: 40,
            presence_penalty: 0.0,
            repeat_penalty: 1.0,
            repeat_last_n: 64,
            // The chat's own size: another size makes the engine load the model again.
            num_ctx: tuning.context,
            num_predict: SCENE_TOKENS,
            stop: Vec::new(),
        };
        let params = ChatParams {
            format: Some(&schema),
            ..chat_params(&model, &messages, &options, tuning.think.map(|_| false))
        };
        let mut scene = None;
        for _ in 0..2 {
            let answer = tokio::select! {
                biased;
                () = token.cancelled() => return Err(Error::Cancelled),
                a = client.complete(&params, SCENE_TIMEOUT) => a,
            };
            match answer {
                Ok(text) => {
                    scene = pictures::scene::parse(&text);
                    if scene.is_some() {
                        break;
                    }
                    log::warn!("the scene answer was not usable");
                }
                Err(e) => log::warn!("the scene step failed: {e}"),
            }
        }
        let mut scene = scene.unwrap_or_else(|| pictures::scene::fallback(last, wish));
        // Small models fill the outfit fields even when nothing changed.
        let said: String = lines.iter().map(|(_, text)| format!("{text} ")).collect();
        pictures::scene::keep_said_outfits(&mut scene, &format!("{said} {wish}"));
        Ok(PictureScene {
            scene,
            character,
            persona,
            chat_model: model,
            message_id: recent.last().map(|m| m.id),
            minor_mentioned,
        })
    }

    /// The people of a chat as they are now, and its chat model.
    pub async fn picture_people(
        &self,
        chat_id: i64,
    ) -> Result<(Character, Option<Persona>, String)> {
        let chat = self.db.chat(chat_id).await?;
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(&chat).await?;
        let model = self.model_for(&chat, &character);
        Ok((character, persona, model))
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

    /// Streams, checks and saves a reply to the last message of `history`.
    /// A reply that fails the checks gets one silent regenerate.
    async fn reply(
        &self,
        chat: &Chat,
        history: &[Message],
        plan: ReplyPlan,
        cancel: &CancellationToken,
        mut progress: impl FnMut(Progress<'_>),
    ) -> Result<ReplyResult> {
        let client = self.engine.client()?;
        let Some(parent) = history.last() else {
            return Err(Error::Invalid("the chat is empty".into()));
        };
        if !matches!(parent.role.as_str(), "user" | "ooc" | "system") {
            return Err(Error::Invalid(
                "the last message is not from the user".into(),
            ));
        }
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(chat).await?;
        let style = plan
            .style
            .unwrap_or_else(|| StyleSettings::of(&chat.reply_style, &chat.reply_custom));
        let model = self.model_for(chat, &character);
        let tuning = self.engine.tuning_for(&model);
        let recent: Vec<&str> = history
            .iter()
            .rev()
            .filter(|m| m.role == "character")
            .take(3)
            .map(|m| m.content.as_str())
            .collect();

        let user_name = persona
            .as_ref()
            .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str());
        let old_names = self.old_user_names(history, user_name).await;
        let open_thread = if style.initiative == presets::Initiative::FollowMe {
            None
        } else {
            self.open_thread(
                &client,
                &model,
                &tuning,
                &character.name,
                user_name,
                history,
            )
            .await
        };
        let memories = self.db.memories(chat.id).await?;
        let recall = self.recall(chat, &memories, history).await?;
        let shown = memory::window(history, chat.context_start_id, &memories);
        let mut extra: Option<String> = None;
        let mut hint = None;
        let (outcome, content, prompt) = loop {
            let mature =
                crate::content::mature_chat(&self.db, &character, persona.as_ref()).await?;
            let prompt = prompt::build(&PromptInput {
                character: &character,
                persona: persona.as_ref(),
                tone: &chat.tone,
                tone_custom: &chat.tone_custom,
                style,
                history: shown,
                push_character: plan.push_character,
                open_thread: open_thread.as_deref(),
                recall: Some(&recall),
                extra_reminder: extra.as_deref(),
                continuation: false,
                tuning: &tuning,
                old_user_names: &old_names,
                mature,
            });
            // The last try is kept even when it repeats, so it streams freely.
            let watch: &[&str] = if extra.is_none() { &recent } else { &[] };
            let (outcome, repeated) = stream_reply(
                &client,
                &chat_params(&model, &prompt.messages, &prompt.options, tuning.think),
                cancel,
                &prompt.user_name,
                watch,
                &mut progress,
            )
            .await?;
            log_tokens(&prompt.messages, outcome.prompt_eval_count);
            let content = guard::clean(
                &outcome.text,
                &character.name,
                &prompt.user_name,
                outcome.cut_at_cap,
                false,
            );
            refuse_crossing(&outcome.text)?;
            let problem = if outcome.stopped {
                None
            } else if repeated {
                Some(Problem::Repetition)
            } else if content.is_empty() {
                Some(Problem::CharacterBreak)
            } else {
                guard::check(&content, &recent)
            };
            match (problem, extra.is_some()) {
                (Some(p), false) => {
                    log::info!(
                        "silent regenerate: {p:?}{}",
                        if repeated { " (at the start)" } else { "" }
                    );
                    progress(Progress::Restart);
                    extra = Some(p.reminder().replace("{{char}}", &character.name));
                }
                (Some(p), true) => {
                    hint = Some(match p {
                        Problem::CharacterBreak => OUT_OF_CHARACTER_HINT,
                        Problem::Repetition => REPEAT_HINT,
                    });
                    break (outcome, content, prompt);
                }
                (None, _) => break (outcome, content, prompt),
            }
        };
        if content.is_empty() {
            return if outcome.stopped {
                Ok(ReplyResult {
                    message: None,
                    hint: None,
                })
            } else {
                Err(empty_reply(&outcome.text))
            };
        }
        let sampler = serde_json::to_string(&prompt.options)?;
        let saved = self
            .db
            .append_message(&NewMessage {
                chat_id: chat.id,
                parent_id: Some(parent.id),
                role: "character",
                content: &content,
                raw_content: Some(&outcome.text),
                token_count: outcome.eval_count,
                model: Some(&model),
                prompt_version: Some(PROMPT_VERSION),
                sampler_json: Some(&sampler),
                ..Default::default()
            })
            .await?;
        Ok(ReplyResult {
            message: Some(saved),
            hint,
        })
    }
}

/// Streams a reply and stops it where the model starts to write the user's
/// part. That line never reaches the screen, so the text on the screen is
/// the text that the guard keeps, and the engine does not write what the
/// guard throws away. With `recent` replies, the start of the reply also
/// waits for the repeat check, and a copied opening stops the stream before
/// it shows. The second value is true then. `stopped` stays true only when
/// the user stopped the reply.
async fn stream_reply(
    client: &crate::ollama::Client,
    params: &ChatParams<'_>,
    cancel: &CancellationToken,
    user_name: &str,
    recent: &[&str],
    progress: &mut impl FnMut(Progress<'_>),
) -> Result<(crate::ollama::ChatOutcome, bool)> {
    let stop = cancel.child_token();
    let mut watch = guard::ReplyWatch::new(user_name, recent);
    let mut line = LineWatch::new("");
    let mut outcome = client
        .chat(params, REPLY_TIMEOUT, &stop, |t| {
            if line.crossed {
                return;
            }
            let shown = watch.push(t);
            if !shown.is_empty() {
                if !line.allows(&shown) {
                    stop.cancel();
                    return;
                }
                progress(Progress::Text(&shown));
            }
            if watch.stop() {
                stop.cancel();
            }
        })
        .await?;
    let rest = watch.finish();
    if !rest.is_empty() && line.allows(&rest) {
        progress(Progress::Text(&rest));
    }
    outcome.stopped = cancel.is_cancelled();
    Ok((outcome, watch.repeated()))
}

fn chat_params<'a>(
    model: &'a str,
    messages: &'a [ChatMessage],
    options: &'a Options,
    think: Option<bool>,
) -> ChatParams<'a> {
    ChatParams {
        model,
        messages,
        options,
        keep_alive: KEEP_ALIVE,
        think,
        format: None,
    }
}

/// Logs the estimated and the real prompt size, so the estimate can be tuned.
fn log_tokens(messages: &[ChatMessage], real: Option<i64>) {
    let estimate: u32 = messages
        .iter()
        .map(|m| prompt::estimate_tokens(&m.content))
        .sum();
    if let Some(real) = real {
        log::info!("prompt tokens: estimate {estimate}, engine {real}");
    }
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .ends_with(['.', '!', '?', '*', '"', '\u{201D}'])
}

/// Adds continued text to a reply, with a space or a new paragraph between them.
fn join_continuation(old: &str, new: &str) -> String {
    let new_part = new.trim_start();
    if new_part.is_empty() {
        return old.to_owned();
    }
    if new.starts_with(char::is_whitespace) || !ends_sentence(old) {
        format!("{old}{new}")
    } else {
        format!("{}\n\n{new_part}", old.trim_end())
    }
}

fn position(path: &[Message], message_id: i64) -> Result<usize> {
    path.iter()
        .position(|m| m.id == message_id)
        .ok_or(Error::Invalid(
            "the message is not on the current branch".into(),
        ))
}

/// `/ooc text` or `((text))` is an out-of-character note.
fn split_ooc(text: &str) -> Result<(&'static str, String)> {
    let note = match text.get(..4) {
        Some(head)
            if head.eq_ignore_ascii_case("/ooc")
                && (text.len() == 4 || text[4..].starts_with(char::is_whitespace)) =>
        {
            Some(text[4..].trim().to_owned())
        }
        _ if text.starts_with("((") && text.ends_with("))") && text.len() >= 4 => {
            Some(text[2..text.len() - 2].trim().to_owned())
        }
        _ => None,
    };
    match note {
        Some(n) if n.is_empty() => Err(Error::Invalid("the note is empty".into())),
        Some(n) => Ok(("ooc", n)),
        None => Ok(("user", text.to_owned())),
    }
}

/// Trims the text, removes control characters, and checks the length.
pub fn clean_input(text: &str) -> Result<String> {
    clean_text(text, MAX_MESSAGE_CHARS)
}

/// `clean_input` with another length limit, for example for imported replies.
pub fn clean_text(text: &str, max: usize) -> Result<String> {
    let text: String = text
        .trim()
        .chars()
        .filter(|c| (!c.is_control() || matches!(c, '\n' | '\t')) && !is_bidi_control(*c))
        .collect();
    if text.is_empty() {
        return Err(Error::Invalid("the message is empty".into()));
    }
    if text.chars().count() > max {
        return Err(Error::Invalid(format!(
            "the message is longer than {max} characters"
        )));
    }
    // The hard limit holds for what the user writes too, so a story never
    // stores such text, and the model never reads it.
    if crate::pictures::safety::reply_crosses_line(&text) {
        log::warn!("a message was refused: explicit words next to a minor");
        return Err(Error::Rejected {
            message: "Unstrung does not take sexual content that involves anyone under 18. Change the message.".into(),
            why: "explicit words next to a minor".into(),
        });
    }
    Ok(text)
}

/// One line of plain text: no control or direction characters, cut to `max` characters.
pub fn clean_line(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Right-to-left overrides and isolates can make text look like something else.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod crossing_tests {
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
            let head = "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n";
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
}
