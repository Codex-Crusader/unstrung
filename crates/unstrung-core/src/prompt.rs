//! Builds the prompt for one reply.
//!
//! Order: system rules, the character card, the persona, example dialogue,
//! recent messages, and an end reminder. Each part has a token budget.
//! The rules are always first and the reminder is always last, whatever a card says.
//! Memories, the summary and world info come in later phases.

use crate::db::{Character, Message, Persona};
use crate::ollama::{ChatMessage, Options};
use crate::presets::{Length, StyleSettings};

/// Saved with each reply. Raise it when the prompt text or order changes.
pub const PROMPT_VERSION: i64 = 3;

/// The name of the user in a chat with no persona.
pub const NO_PERSONA_NAME: &str = "User";

pub const CONTEXT_TOKENS: u32 = 8192;
const REPLY_MARGIN_TOKENS: u32 = 64;
const CARD_SHARE: u32 = 20;
const PERSONA_SHARE: u32 = 5;
const EXAMPLES_SHARE: u32 = 10;
/// Average characters per token. Low on purpose, so the estimate runs high.
const CHARS_PER_TOKEN: f32 = 3.5;

const SYSTEM_RULES: &str = "\
You write one side of a roleplay chat.
1. You are {{char}}. Stay in the story at all times.
2. Write only what {{char}} says, does and thinks. {{user}} writes their own words and actions.
3. React to what {{user}} just wrote before you add anything new.
4. Keep the facts about {{char}}, {{user}} and the world consistent with what came before.
5. Follow the reply style and tone at the end of this prompt.
6. Keep content non-explicit. Never write sexual content that involves anyone under 18. \
If a scene goes there, {{char}} steers it away, in character.";

/// The first line of the end reminder. The guard removes it if a reply copies it.
pub const REMINDER_HEAD: &str = "[Reply guide for {{char}}. Not part of the story.]";
const STORY_NOTE: &str = "Story note, not said by anyone:";
const OPENING_WORDS: usize = 5;

const OOC_NOTE: &str = "(Note from {{user}}, not part of the story: {{note}}\n\
Follow it, and continue the story in character.)";

pub struct PromptInput<'a> {
    pub character: &'a Character,
    pub persona: Option<&'a Persona>,
    pub tone: &'a str,
    pub tone_custom: &'a str,
    pub style: StyleSettings,
    /// The path from the first message to the head.
    pub history: &'a [Message],
    /// "More in character": the anchor is repeated and the examples are always in.
    pub push_character: bool,
    /// An extra line for the end reminder, for a silent regenerate.
    pub extra_reminder: Option<&'a str>,
    /// Continue the last message of `history` instead of writing a new one.
    pub continuation: bool,
}

pub struct Prompt {
    pub messages: Vec<ChatMessage>,
    pub options: Options,
    pub user_name: String,
}

/// A rough token count. The real count comes back from the engine and is logged.
pub fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count() as f32;
    (chars / CHARS_PER_TOKEN).ceil() as u32 + 4
}

/// Cuts text to a token budget, at the last line or sentence end before the limit.
fn fit(text: &str, max_tokens: u32) -> String {
    if estimate_tokens(text) <= max_tokens {
        return text.to_owned();
    }
    let max_chars = ((max_tokens.saturating_sub(4)) as f32 * CHARS_PER_TOKEN) as usize;
    let cut: String = text.chars().take(max_chars).collect();
    match cut.rfind(['\n', '.', '!', '?']) {
        Some(end) if end > cut.len() / 2 => cut[..=end].to_owned(),
        _ => cut,
    }
}

struct Names<'a> {
    char: &'a str,
    user: &'a str,
}

impl Names<'_> {
    fn fill(&self, text: &str) -> String {
        text.replace("{{char}}", self.char)
            .replace("{{user}}", self.user)
            .replace("{{Char}}", self.char)
            .replace("{{User}}", self.user)
    }
}

fn section(title: &str, lines: &[(&str, &str)]) -> String {
    let mut out = format!("\n## {title}\n");
    for (label, value) in lines {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if label.is_empty() {
            out.push_str(value);
        } else {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(value);
        }
        out.push('\n');
    }
    out
}

/// True when the examples are much longer than a texting reply.
fn examples_too_long(examples: &str) -> bool {
    let lines: Vec<&str> = examples
        .lines()
        .filter(|l| l.trim_start().starts_with("{{char}}"))
        .collect();
    !lines.is_empty() && lines.iter().map(|l| l.len()).sum::<usize>() / lines.len() > 160
}

/// Adds a message. Two user messages in a row become one, as the engine joins them anyway.
fn push_merged(messages: &mut Vec<ChatMessage>, role: &'static str, content: String) {
    match messages.last_mut() {
        Some(last) if role == "user" && last.role == "user" => {
            last.content.push_str("\n\n");
            last.content.push_str(&content);
        }
        _ => messages.push(ChatMessage { role, content }),
    }
}

/// The first words of the character's last reply.
fn last_opening(history: &[Message]) -> Option<String> {
    let last = history.iter().rev().find(|m| m.role == "character")?;
    let words: Vec<&str> = last
        .content
        .split_whitespace()
        .take(OPENING_WORDS)
        .collect();
    (!words.is_empty()).then(|| words.join(" ").replace('"', "'"))
}

pub fn build(input: &PromptInput<'_>) -> Prompt {
    let character = input.character;
    let user_name = input.persona.map_or(NO_PERSONA_NAME, |p| p.name.as_str());
    let names = Names {
        char: &character.name,
        user: user_name,
    };
    let style = input.style;
    let (tone_text, temperature) = crate::presets::tone(input.tone, input.tone_custom);

    let card_budget = CONTEXT_TOKENS * CARD_SHARE / 100;
    let persona_budget = CONTEXT_TOKENS * PERSONA_SHARE / 100;
    let examples_budget = CONTEXT_TOKENS * EXAMPLES_SHARE / 100;

    // 1 to 3: rules, character card, persona.
    let card = section(
        "You are {{char}}",
        &[
            ("", "These are {{char}}'s own facts."),
            ("Name", &character.name),
            ("Personality", &character.personality),
            ("Scenario", &character.scenario),
            ("Appearance", &character.appearance_tags),
        ],
    );
    let persona = match input.persona {
        Some(p) => section(
            "The person you talk to: {{user}}",
            &[
                (
                    "",
                    "{{user}} is another person. Their traits are not yours.",
                ),
                ("About {{user}}", &p.description),
                ("Appearance", &p.appearance_tags),
            ],
        ),
        None => section(
            "The person you talk to: {{user}}",
            &[(
                "",
                "{{user}} is another person. Their traits are not yours.",
            )],
        ),
    };
    let system = names.fill(SYSTEM_RULES)
        + &fit(&names.fill(&card), card_budget)
        + &fit(&names.fill(&persona), persona_budget);

    // 9: end reminder. It goes in the user role, because chat templates move
    // every system message to the top of the prompt.
    let mut reminder = String::from(REMINDER_HEAD);
    reminder.push('\n');
    let anchor = character.voice_anchor.trim();
    if !anchor.is_empty() {
        reminder.push_str(&format!("Voice of {{{{char}}}}: {anchor}\n"));
        if input.push_character {
            reminder.push_str("Stay true to this voice in every line.\n");
        }
    }
    reminder.push_str(&format!(
        "Tone: {tone_text}\nReply style: {}\n",
        style.instruction()
    ));
    if let Some(opening) = last_opening(input.history) {
        reminder.push_str(&format!(
            "Your last reply began with \"{opening}\". Begin this one in a different way.\n"
        ));
    }
    if let Some(extra) = input.extra_reminder {
        reminder.push_str(extra);
        reminder.push('\n');
    }
    reminder.push_str(
        "Do not write what {{user}} says, does or feels. Stop when it is {{user}}'s turn.\n\
         Reply as {{char}} only.",
    );
    let reminder = names.fill(&reminder);

    // 7: example dialogue.
    let raw_examples = character.example_dialogue.trim();
    let use_examples = !raw_examples.is_empty()
        && (input.push_character
            || !(style.length == Length::VeryShort && examples_too_long(raw_examples)));
    let examples = use_examples.then(|| {
        fit(
            &names.fill(&section(
                "Example dialogue (shows how {{char}} talks)",
                &[("", raw_examples)],
            )),
            examples_budget,
        )
    });

    let num_predict = style.num_predict();
    let reserved = u32::try_from(num_predict).unwrap_or_default() + REPLY_MARGIN_TOKENS;
    let mut budget = CONTEXT_TOKENS
        .saturating_sub(reserved)
        .saturating_sub(estimate_tokens(&system))
        .saturating_sub(estimate_tokens(&reminder));
    let forced_examples = match &examples {
        Some(ex) if input.push_character => {
            budget = budget.saturating_sub(estimate_tokens(ex));
            true
        }
        _ => false,
    };

    // 8: recent messages, newest first until the budget is used.
    let (history, prefill) = match (input.continuation, input.history.split_last()) {
        (true, Some((last, rest))) if last.role == "character" => (rest, Some(last)),
        _ => (input.history, None),
    };
    if let Some(p) = prefill {
        budget = budget.saturating_sub(estimate_tokens(&p.content));
    }
    let mut kept = Vec::new();
    for message in history.iter().rev() {
        let cost = estimate_tokens(&message.content);
        if cost > budget {
            break;
        }
        budget -= cost;
        kept.push(message);
    }
    kept.reverse();

    // Examples are dropped first when space is short.
    let system = match examples {
        Some(ex) if forced_examples || estimate_tokens(&ex) <= budget => system + &ex,
        _ => system,
    };

    let mut messages = vec![ChatMessage {
        role: "system",
        content: system,
    }];
    for message in kept {
        let (role, content) = match message.role.as_str() {
            "character" => ("assistant", message.content.clone()),
            "user" => ("user", message.content.clone()),
            "ooc" => (
                "user",
                names
                    .fill(OOC_NOTE)
                    .replace("{{note}}", message.content.trim()),
            ),
            "system" => ("user", format!("({STORY_NOTE} {})", message.content.trim())),
            _ => continue,
        };
        push_merged(&mut messages, role, content);
    }
    push_merged(&mut messages, "user", reminder);
    // The engine continues an assistant message that comes last.
    if let Some(p) = prefill {
        messages.push(ChatMessage {
            role: "assistant",
            content: p.content.clone(),
        });
    }

    let options = Options {
        temperature,
        min_p: 0.05,
        top_p: 1.0,
        repeat_penalty: 1.08,
        repeat_last_n: 256,
        num_ctx: CONTEXT_TOKENS,
        num_predict,
        stop: vec![format!("\n{user_name}:"), format!("{user_name}:")],
    };
    Prompt {
        messages,
        options,
        user_name: user_name.to_owned(),
    }
}

#[cfg(test)]
mod tests;
