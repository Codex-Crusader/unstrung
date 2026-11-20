//! Character growth in a chat: the feelings line for the next reply, and the
//! critic that appraises each reply after it is saved. Off unless the
//! `growth::ask::SWITCH_KEY` meta value is "on".

use super::*;
use crate::growth::{self, Appraisal, Genome, ask};

/// The critic answer is short JSON.
const CRITIC_TOKENS: i32 = 160;
const CRITIC_TIMEOUT: Duration = Duration::from_secs(60);

/// The prompt that the engine read for the newest reply, and the raw reply.
/// The critic continues it, so the engine reads only the critic question.
pub(super) struct LastPrompt {
    pub message_id: i64,
    pub messages: Vec<ChatMessage>,
    pub raw_reply: String,
}

/// What one critic call cost, for the quality tool.
#[derive(Debug, Clone, Copy, Default)]
pub struct CriticCost {
    pub millis: u128,
    pub prompt_tokens: Option<i64>,
    pub prompt_read_ms: Option<i64>,
    pub answer_tokens: Option<i64>,
    /// True when the stored reply prompt was used, false for the short fallback.
    pub continued: bool,
    /// False when the answer was not the JSON asked for (counted as no change).
    pub parsed: bool,
}

impl ChatService {
    pub async fn growth_on(&self) -> bool {
        matches!(self.db.meta(ask::SWITCH_KEY).await, Ok(Some(v)) if v == "on")
    }

    /// The feelings line for a reply to the end of `history`. `None` when
    /// growth is off or nothing changed enough to say.
    pub(super) async fn feelings(
        &self,
        chat: &Chat,
        character: &Character,
        persona: Option<&Persona>,
        history: &[Message],
    ) -> Result<Option<String>> {
        if !self.growth_on().await {
            return Ok(None);
        }
        let Some((_, genome)) = self.db.genome(character.id).await? else {
            return Ok(None);
        };
        let rows = self.db.growth_states(chat.id).await?;
        let romance = crate::content::adult_people(character, persona);
        let text = growth::on_path(&rows, history, &genome).describe(&genome, romance);
        Ok((!text.is_empty()).then_some(text))
    }

    /// Appraises the newest reply of a chat, when growth is on and the reply
    /// has no state yet. The app runs it after each reply. It waits while a
    /// reply or a picture runs, because the engine answers one at a time.
    pub async fn appraise(&self, chat_id: i64) -> Result<Option<CriticCost>> {
        if self.idle().is_err() || self.closing.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(None);
        }
        let chat = self.db.chat(chat_id).await?;
        let path = self.db.path_to_head(chat_id).await?;
        self.appraise_newest(&chat, &path).await
    }

    /// Appraises the newest reply in `path` that answers a user message,
    /// once. A reply waits for this, so it never uses an old state.
    pub(super) async fn appraise_newest(
        &self,
        chat: &Chat,
        path: &[Message],
    ) -> Result<Option<CriticCost>> {
        if !self.growth_on().await {
            return Ok(None);
        }
        let _one = self.growth_lock.lock().await;
        let Some(at) = path.iter().rposition(|m| m.role == "character") else {
            return Ok(None);
        };
        let reply = &path[at];
        let Some(said) = at
            .checked_sub(1)
            .map(|i| &path[i])
            .filter(|m| m.role == "user")
        else {
            return Ok(None);
        };
        let rows = self.db.growth_states(chat.id).await?;
        if rows.iter().any(|(id, _, _)| *id == reply.id) {
            return Ok(None);
        }
        let character = self.db.character(chat.character_id).await?;
        let persona = self.persona_of(chat).await?;
        let user_name = persona
            .as_ref()
            .map_or(prompt::NO_PERSONA_NAME, |p| p.name.as_str());
        let model = self.model_for(chat, &character);
        let tuning = self.engine.tuning_for(&model);
        let genome = self.genome_for(&character, &model, &tuning).await?;

        let question = ChatMessage {
            role: "user",
            images: Vec::new(),
            content: ask::critic_question(&character.name, user_name, &said.content),
        };
        let stored = self
            .last_prompt
            .lock()
            .ok()
            .and_then(|mut p| p.take())
            .filter(|p| p.message_id == reply.id);
        let continued = stored.is_some();
        let messages = match stored {
            Some(p) => {
                let mut m = p.messages;
                m.push(ChatMessage {
                    role: "assistant",
                    images: Vec::new(),
                    content: p.raw_reply,
                });
                m.push(question);
                m
            }
            // After a restart: a short prompt with the last exchange only.
            None => vec![
                ChatMessage {
                    role: "system",
                    images: Vec::new(),
                    content: format!(
                        "A roleplay chat. {} is the character. {user_name} is the user.",
                        character.name
                    ),
                },
                ChatMessage {
                    role: "user",
                    images: Vec::new(),
                    content: format!(
                        "{user_name}: {}\n\n{}: {}\n\n{}",
                        said.content, character.name, reply.content, question.content
                    ),
                },
            ],
        };
        let schema = ask::critic_schema();
        let options = quiet_options(tuning.context, CRITIC_TOKENS);
        let params = ChatParams {
            format: Some(&schema),
            ..chat_params(&model, &messages, &options, tuning.think.map(|_| false))
        };
        let started = std::time::Instant::now();
        let client = self.engine.client()?;
        let outcome = client
            .chat(&params, CRITIC_TIMEOUT, &CancellationToken::new(), |_| {})
            .await?;
        let parsed = ask::parse_appraisal(&outcome.text);
        let cost = CriticCost {
            millis: started.elapsed().as_millis(),
            prompt_tokens: outcome.prompt_eval_count,
            prompt_read_ms: outcome.prompt_eval_duration.map(|d| d / 1_000_000),
            answer_tokens: outcome.eval_count,
            continued,
            parsed: parsed.is_some(),
        };
        // A broken answer counts as no change, so the reply is not asked
        // about again on every turn.
        let appraisal = parsed.unwrap_or_else(|| {
            log::warn!("growth: the critic answer was not usable");
            Appraisal::default()
        });
        let before = growth::on_path(&rows, &path[..at], &genome);
        let romance = crate::content::adult_people(&character, persona.as_ref());
        let after = before.after(&genome, &appraisal, romance);
        self.db
            .add_growth_state(chat.id, reply.id, &after, &appraisal)
            .await?;
        // Numbers only: the reason is chat text.
        let d = after.drives;
        log::info!(
            "growth: critic in {} ms ({}), read {:?} tokens in {:?} ms; valence {}, \
             trust {:.2}, closeness {:.2}, respect {:.2}, romance {:.2}",
            cost.millis,
            if continued { "continued" } else { "short" },
            cost.prompt_tokens,
            cost.prompt_read_ms,
            appraisal.valence,
            d.trust,
            d.closeness,
            d.respect,
            d.romance,
        );
        Ok(Some(cost))
    }

    /// The character's genome for its current card, made once per card
    /// version. When the model gives no usable answer, an even genome is
    /// used and nothing is saved, so the next appraisal asks again.
    async fn genome_for(
        &self,
        character: &Character,
        model: &str,
        tuning: &crate::catalog::Tuning,
    ) -> Result<Genome> {
        let hash = ask::card_hash(character);
        if let Some((saved, genome)) = self.db.genome(character.id).await?
            && saved == hash
        {
            return Ok(genome);
        }
        let messages = [ChatMessage {
            role: "user",
            images: Vec::new(),
            content: ask::genome_question(character),
        }];
        let schema = ask::genome_schema();
        let options = quiet_options(tuning.context, 80);
        let params = ChatParams {
            format: Some(&schema),
            ..chat_params(model, &messages, &options, tuning.think.map(|_| false))
        };
        let answer = self
            .engine
            .client()?
            .complete(&params, CRITIC_TIMEOUT)
            .await?;
        let Some(genome) = ask::parse_genome(&answer) else {
            log::warn!("growth: the genome answer was not usable");
            return Ok(Genome::PLAIN);
        };
        self.db.set_genome(character.id, &hash, &genome).await?;
        log::info!(
            "growth: new genome for character {}: {genome:?}",
            character.id
        );
        Ok(genome)
    }
}

/// Options for a helper answer: no randomness, the chat's own context size
/// (another size makes the engine load the model again).
pub(super) fn quiet_options(num_ctx: u32, num_predict: i32) -> Options {
    Options {
        temperature: 0.0,
        min_p: 0.0,
        top_p: 1.0,
        top_k: 1,
        presence_penalty: 0.0,
        repeat_penalty: 1.0,
        repeat_last_n: 64,
        num_ctx,
        num_predict,
        stop: Vec::new(),
    }
}
