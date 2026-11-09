//! Writing replies: send, edit, retry, regenerate, continue, and the stream from the model.

use super::*;

impl ChatService {
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
    pub(super) async fn new_greeting(
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

    /// Asks the model what the user still waits for from earlier in the chat.
    /// A failure only costs the hint, so it is logged and skipped.
    pub(super) async fn open_thread(
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

    /// Streams, checks and saves a reply to the last message of `history`.
    /// A reply that fails the checks gets one silent regenerate.
    pub(super) async fn reply(
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
pub(super) async fn stream_reply(
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

pub(super) fn chat_params<'a>(
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
pub(super) fn log_tokens(messages: &[ChatMessage], real: Option<i64>) {
    let estimate: u32 = messages
        .iter()
        .map(|m| prompt::estimate_tokens(&m.content))
        .sum();
    if let Some(real) = real {
        log::info!("prompt tokens: estimate {estimate}, engine {real}");
    }
}

pub(super) fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .ends_with(['.', '!', '?', '*', '"', '\u{201D}'])
}

/// Adds continued text to a reply, with a space or a new paragraph between them.
pub(super) fn join_continuation(old: &str, new: &str) -> String {
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
