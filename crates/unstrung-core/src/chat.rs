//! Chat logic: replies, the message tree, personas, tone and reply style.
//!
//! Messages form a tree. Rewind, regenerate, edit and branch all move the head
//! of the chat, so later messages become hidden branches and nothing is lost.

use crate::db::{Character, Chat, ChatSummary, Db, Message, NewMessage, Persona, now_ms};
use crate::engine::{Engine, KEEP_ALIVE};
use crate::error::{Error, Result};
use crate::guard::{self, Problem};
use crate::ollama::{ChatMessage, ChatParams, Options};
use crate::presets::{self, StyleSettings};
use crate::prompt::{self, PROMPT_VERSION, PromptInput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Longest user message, in characters.
pub const MAX_MESSAGE_CHARS: usize = 8000;
/// "No reply after 60 seconds."
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// The end-reminder line for "Continue".
const CONTINUE_REMINDER: &str =
    "Continue your last reply with one more paragraph. Do not repeat what you already wrote.";
/// Shown under a reply that failed the checks twice.
pub const OUT_OF_CHARACTER_HINT: &str = "Not in character? Regenerate.";

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
}

/// Clears the "reply in progress" slot when the reply ends in any way.
struct ReplyGuard<'a>(&'a Mutex<Option<CancellationToken>>);

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
        }
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
        Ok(ChatView {
            chat,
            character,
            persona,
            messages,
            branches,
        })
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
        let character = self.db.character(character_id).await?;
        if let Some(id) = persona_id {
            self.db.persona(id).await?;
        }
        let id = self
            .db
            .create_chat(&character, persona_id, "", true)
            .await?;
        self.view(id).await
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
        let request = Message {
            id: 0,
            chat_id: chat.id,
            parent_id: None,
            role: "user".into(),
            persona_id: chat.persona_id,
            content: format!(
                "(Story note, not said by anyone: the chat starts now. Write the opening                  message of the story as {}.)",
                character.name
            ),
            flagged_out_of_character: false,
            model: None,
            created_at: 0,
        };
        let note = format!(
            "This is the first message of a new chat. The usual opening is below. Keep its              setting and situation, but write a new opening in your own words. Do not copy it.
             Usual opening: \"{}\"",
            greeting.content
        );
        let mut extra = note.clone();
        let mut retried = false;
        let (outcome, content, prompt, hint) = loop {
            let prompt = prompt::build(&PromptInput {
                character: &character,
                persona: persona.as_ref(),
                tone: &chat.tone,
                tone_custom: &chat.tone_custom,
                style,
                history: std::slice::from_ref(&request),
                push_character: plan.push_character,
                extra_reminder: Some(&extra),
                continuation: false,
            });
            let outcome = client
                .chat(
                    &chat_params(&model, &prompt.messages, &prompt.options),
                    REPLY_TIMEOUT,
                    cancel,
                    |t| progress(Progress::Text(t)),
                )
                .await?;
            let content = guard::clean(
                &outcome.text,
                &character.name,
                &prompt.user_name,
                outcome.cut_at_cap,
                false,
            );
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
                Err(Error::Engine("the reply was empty".into()))
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
        let prompt = prompt::build(&PromptInput {
            character: &character,
            persona: persona.as_ref(),
            tone: &chat.tone,
            tone_custom: &chat.tone_custom,
            style: StyleSettings::of(&chat.reply_style, &chat.reply_custom),
            history: &history,
            push_character: false,
            extra_reminder: Some(CONTINUE_REMINDER),
            continuation: mid_sentence,
        });
        let model = self.model_for(&chat, &character);
        let outcome = client
            .chat(
                &chat_params(&model, &prompt.messages, &prompt.options),
                REPLY_TIMEOUT,
                &cancel,
                |t| progress(Progress::Text(t)),
            )
            .await?;
        let joined = join_continuation(&last.content, &outcome.text);
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
            extra_reminder: None,
            continuation: false,
        });
        let model = self.engine.model().to_owned();
        let outcome = client
            .chat(
                &chat_params(&model, &prompt.messages, &prompt.options),
                REPLY_TIMEOUT,
                &cancel,
                |t| progress(Progress::Text(t)),
            )
            .await?;
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
                content: "You help write roleplay characters. Answer with plain text only.".into(),
            },
            ChatMessage {
                role: "user",
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
            repeat_penalty: 1.08,
            repeat_last_n: 64,
            num_ctx: 2048,
            num_predict: 120,
            stop: Vec::new(),
        };
        let model = self.engine.model().to_owned();
        let text = client
            .complete(&chat_params(&model, &messages, &options), REPLY_TIMEOUT)
            .await?;
        Ok(clean_line(text.trim().trim_matches('"'), 400))
    }

    fn begin(&self) -> Result<CancellationToken> {
        let mut slot = self.current.lock().map_err(|_| Error::Busy)?;
        if slot.is_some() {
            return Err(Error::Busy);
        }
        let token = CancellationToken::new();
        *slot = Some(token.clone());
        Ok(token)
    }

    /// Tree changes wait until no reply is being written.
    fn idle(&self) -> Result<()> {
        match self.current.lock() {
            Ok(slot) if slot.is_none() => Ok(()),
            _ => Err(Error::Busy),
        }
    }

    fn model_for(&self, chat: &Chat, character: &Character) -> String {
        chat.chat_model
            .clone()
            .or_else(|| character.model_override.clone())
            .unwrap_or_else(|| self.engine.model().to_owned())
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
        let recent: Vec<&str> = history
            .iter()
            .rev()
            .filter(|m| m.role == "character")
            .take(3)
            .map(|m| m.content.as_str())
            .collect();

        let mut extra: Option<String> = None;
        let mut hint = None;
        let (outcome, content, prompt) = loop {
            let prompt = prompt::build(&PromptInput {
                character: &character,
                persona: persona.as_ref(),
                tone: &chat.tone,
                tone_custom: &chat.tone_custom,
                style,
                history,
                push_character: plan.push_character,
                extra_reminder: extra.as_deref(),
                continuation: false,
            });
            let outcome = client
                .chat(
                    &chat_params(&model, &prompt.messages, &prompt.options),
                    REPLY_TIMEOUT,
                    cancel,
                    |t| progress(Progress::Text(t)),
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
            let problem = if outcome.stopped {
                None
            } else if content.is_empty() {
                Some(Problem::CharacterBreak)
            } else {
                guard::check(&content, &recent)
            };
            match (problem, extra.is_some()) {
                (Some(p), false) => {
                    log::info!("silent regenerate: {p:?}");
                    progress(Progress::Restart);
                    extra = Some(p.reminder().replace("{{char}}", &character.name));
                }
                (Some(_), true) => {
                    hint = Some(OUT_OF_CHARACTER_HINT);
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
                Err(Error::Engine("the reply was empty".into()))
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

fn chat_params<'a>(
    model: &'a str,
    messages: &'a [ChatMessage],
    options: &'a Options,
) -> ChatParams<'a> {
    ChatParams {
        model,
        messages,
        options,
        keep_alive: KEEP_ALIVE,
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
    let text: String = text
        .trim()
        .chars()
        .filter(|c| (!c.is_control() || matches!(c, '\n' | '\t')) && !is_bidi_control(*c))
        .collect();
    if text.is_empty() {
        return Err(Error::Invalid("the message is empty".into()));
    }
    if text.chars().count() > MAX_MESSAGE_CHARS {
        return Err(Error::Invalid(format!(
            "the message is longer than {MAX_MESSAGE_CHARS} characters"
        )));
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

    #[test]
    fn input_is_checked() {
        assert!(clean_input("   ").is_err());
        assert!(clean_input(&"a".repeat(MAX_MESSAGE_CHARS + 1)).is_err());
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
