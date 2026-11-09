//! Helps outside the chat: the test reply, suggestions for a new character, and picture scenes.

use super::*;

impl ChatService {
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
}
