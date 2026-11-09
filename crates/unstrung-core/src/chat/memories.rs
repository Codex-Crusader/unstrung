//! What a chat remembers: the memories, the summary and the passes that update them.

use super::*;

impl ChatService {
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

    pub(super) async fn memory_passes(
        &self,
        chat_id: i64,
        token: &CancellationToken,
    ) -> Result<()> {
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
    pub(super) async fn ask_pass(
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
}
