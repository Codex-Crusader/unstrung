//! Chat logic: send a message, stream the reply, stop, retry.

use crate::db::{Character, Chat, Db, Message, NewMessage};
use crate::engine::{Engine, KEEP_ALIVE};
use crate::error::{Error, Result};
use crate::ollama::ChatParams;
use crate::prompt::{self, PROMPT_VERSION};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Longest user message, in characters.
pub const MAX_MESSAGE_CHARS: usize = 8000;
/// "No reply after 60 seconds."
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct ChatView {
    pub chat: Chat,
    pub character: Character,
    pub messages: Vec<Message>,
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

    /// The latest chat with a character, or a new one that starts with the greeting.
    pub async fn open_chat(&self, character_id: i64) -> Result<ChatView> {
        let character = self.db.character(character_id).await?;
        let chat = match self.db.latest_chat(character_id).await? {
            Some(chat) => chat,
            None => self.db.create_chat(&character).await?,
        };
        let messages = self.db.path_to_head(chat.id).await?;
        Ok(ChatView {
            chat,
            character,
            messages,
        })
    }

    /// Saves the user's message, then streams the reply to `on_text`.
    /// Returns the saved user message and the saved reply.
    pub async fn send(
        &self,
        chat_id: i64,
        text: &str,
        on_user_saved: impl FnOnce(&Message),
        on_text: impl FnMut(&str),
    ) -> Result<Option<Message>> {
        let text = clean_input(text)?;
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
                role: "user",
                content: &text,
                ..Default::default()
            })
            .await?;
        on_user_saved(&user);
        self.reply(chat_id, &cancel, on_text).await
    }

    /// Writes a reply to the head message again, for example after an error.
    pub async fn retry(&self, chat_id: i64, on_text: impl FnMut(&str)) -> Result<Option<Message>> {
        let cancel = self.begin()?;
        let _guard = ReplyGuard(&self.current);
        self.reply(chat_id, &cancel, on_text).await
    }

    /// Stops the reply in progress. The text so far is kept.
    pub fn stop(&self) {
        if let Ok(slot) = self.current.lock()
            && let Some(token) = slot.as_ref()
        {
            token.cancel();
        }
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

    /// Streams and saves a reply to the head of the chat.
    /// Returns `None` when the user stopped it before any text came.
    async fn reply(
        &self,
        chat_id: i64,
        cancel: &CancellationToken,
        on_text: impl FnMut(&str),
    ) -> Result<Option<Message>> {
        let client = self.engine.client()?;
        let chat = self.db.chat(chat_id).await?;
        let character = self.db.character(chat.character_id).await?;
        let history = self.db.path_to_head(chat_id).await?;
        if history.last().is_none_or(|m| m.role != "user") {
            return Err(Error::Invalid(
                "the last message is not from the user".into(),
            ));
        }
        let model = chat
            .chat_model
            .clone()
            .or_else(|| character.model_override.clone())
            .unwrap_or_else(|| self.engine.model().to_owned());
        let prompt = prompt::build(&character, &chat, &history);

        let outcome = client
            .chat(
                &ChatParams {
                    model: &model,
                    messages: &prompt.messages,
                    options: &prompt.options,
                    keep_alive: KEEP_ALIVE,
                },
                REPLY_TIMEOUT,
                cancel,
                on_text,
            )
            .await?;
        let content = prompt::clean_reply(&outcome.text, &character.name, outcome.cut_at_cap);
        if content.is_empty() {
            return if outcome.stopped {
                Ok(None)
            } else {
                Err(Error::Engine("the reply was empty".into()))
            };
        }
        let sampler = serde_json::to_string(&prompt.options)?;
        let saved = self
            .db
            .append_message(&NewMessage {
                chat_id,
                parent_id: history.last().map(|m| m.id),
                role: "character",
                content: &content,
                raw_content: Some(&outcome.text),
                token_count: outcome.eval_count,
                model: Some(&model),
                prompt_version: Some(PROMPT_VERSION),
                sampler_json: Some(&sampler),
            })
            .await?;
        Ok(Some(saved))
    }
}

/// Trims the text, removes control characters, and checks the length.
fn clean_input(text: &str) -> Result<String> {
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

/// Right-to-left overrides and isolates can make text look like something else.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_checked() {
        assert!(clean_input("   ").is_err());
        assert!(clean_input(&"a".repeat(MAX_MESSAGE_CHARS + 1)).is_err());
        assert_eq!(clean_input(" hi\u{0007}\nthere ").unwrap(), "hi\nthere");
    }
}
