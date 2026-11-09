//! Opening chats, their settings, and the message tree: versions, rewind, delete and branch.

use super::*;

impl ChatService {
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
    pub(super) async fn old_user_names(&self, history: &[Message], now: &str) -> Vec<String> {
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

    pub(super) async fn persona_of(&self, chat: &Chat) -> Result<Option<Persona>> {
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
    pub(super) async fn recall(
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
    pub(super) async fn sync_persona(&self, chat_id: i64) -> Result<()> {
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
}

pub(super) fn position(path: &[Message], message_id: i64) -> Result<usize> {
    path.iter()
        .position(|m| m.id == message_id)
        .ok_or(Error::Invalid(
            "the message is not on the current branch".into(),
        ))
}
