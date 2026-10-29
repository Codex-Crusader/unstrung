//! Builds the prompt for one reply.
//!
//! Phase 1 version: system rules, the character card, example dialogue, recent
//! messages and an end reminder. Phase 2 adds personas, memories and token budgets per part.

use crate::db::{Character, Chat, Message};
use crate::ollama::{ChatMessage, Options};

/// Saved with each reply. Raise it when the prompt text or order changes.
pub const PROMPT_VERSION: i64 = 1;

/// The name of the user until personas exist (Phase 2).
pub const USER_NAME: &str = "User";

pub const CONTEXT_TOKENS: u32 = 8192;
const REPLY_MARGIN_TOKENS: u32 = 64;

const SYSTEM_RULES: &str = "\
You write one side of a roleplay chat.
1. You are {{char}}. Stay in the story at all times.
2. Write only what {{char}} says, does and thinks. {{user}} writes their own words and actions.
3. React to what {{user}} just wrote before you add anything new.
4. Keep the facts about {{char}}, {{user}} and the world consistent with what came before.
5. Follow the reply style and tone at the end of this prompt.
6. Keep content non-explicit. Never write sexual content that involves anyone under 18. \
If a scene goes there, {{char}} steers it away, in character.";

pub struct ReplyStyle {
    pub instruction: &'static str,
    pub num_predict: i32,
}

pub fn reply_style(key: &str) -> ReplyStyle {
    match key {
        "texting" => ReplyStyle {
            instruction: "Reply in 1 to 2 short sentences. Write only what {{char}} says.",
            num_predict: 60,
        },
        "storyteller" => ReplyStyle {
            instruction: "Reply in 2 to 3 paragraphs. Mix dialogue and narration. You can add small events.",
            num_predict: 450,
        },
        "novel" => ReplyStyle {
            instruction: "Reply in 3 to 5 paragraphs, like a page of a book. Use rich narration and inner thoughts, and move the plot forward.",
            num_predict: 600,
        },
        _ => ReplyStyle {
            instruction: "Reply in one short paragraph. Mostly dialogue, with at most one short action in *asterisks*.",
            num_predict: 250,
        },
    }
}

/// Style instruction and temperature for a tone.
pub fn tone(key: &str) -> (&'static str, f32) {
    match key {
        "wholesome" => ("Warm, kind and hopeful.", 0.7),
        "dramatic" => ("Intense, with high stakes and strong feelings.", 0.85),
        "flirty" => ("Playful and teasing.", 0.9),
        "dark" => ("Grim, tense and moody.", 0.8),
        "funny" => ("Light and full of jokes.", 0.95),
        _ => ("Relaxed and natural.", 0.8),
    }
}

pub struct Prompt {
    pub messages: Vec<ChatMessage>,
    pub options: Options,
}

fn fill(text: &str, character: &Character) -> String {
    text.replace("{{char}}", &character.name)
        .replace("{{user}}", USER_NAME)
}

/// A rough token count (about 4 characters per token). Phase 2 counts tokens exactly.
pub fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.chars().count().div_ceil(4))
        .unwrap_or(u32::MAX)
        .saturating_add(4)
}

pub fn build(character: &Character, chat: &Chat, history: &[Message]) -> Prompt {
    let style = reply_style(&chat.reply_style);
    let (tone_text, temperature) = tone(&chat.tone);

    let mut card = format!("{SYSTEM_RULES}\n\n## You are {{{{char}}}}\n");
    for (label, value) in [
        ("Personality", &character.personality),
        ("Scenario", &character.scenario),
    ] {
        if !value.trim().is_empty() {
            card.push_str(&format!("{label}: {}\n", value.trim()));
        }
    }
    card.push_str(
        "\n## The person you talk to: {{user}}\n{{user}} is another person. Their traits are not yours.\n",
    );
    let examples = character.example_dialogue.trim();
    let system = fill(&card, character);
    let examples = (!examples.is_empty()).then(|| {
        fill(
            &format!("\n## Example dialogue (shows how {{{{char}}}} talks)\n{examples}\n"),
            character,
        )
    });

    let mut reminder = String::new();
    if !character.voice_anchor.trim().is_empty() {
        reminder.push_str(&format!(
            "Voice of {{{{char}}}}: {}\n",
            character.voice_anchor.trim()
        ));
    }
    reminder.push_str(&format!(
        "Tone: {tone_text}\nReply style: {}\nStart differently from your last reply.\nReply as {{{{char}}}} only.",
        style.instruction
    ));
    let reminder = fill(&reminder, character);

    let reserved = u32::try_from(style.num_predict).unwrap_or_default() + REPLY_MARGIN_TOKENS;
    let mut budget = CONTEXT_TOKENS
        .saturating_sub(reserved)
        .saturating_sub(estimate_tokens(&system))
        .saturating_sub(estimate_tokens(&reminder));

    // Newest messages first, until the budget is used. Examples are dropped first.
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
    let system = match examples {
        Some(ex) if estimate_tokens(&ex) <= budget => system + &ex,
        _ => system,
    };

    let mut messages = vec![ChatMessage {
        role: "system",
        content: system,
    }];
    for message in kept {
        let role = match message.role.as_str() {
            "character" => "assistant",
            "user" => "user",
            _ => continue,
        };
        messages.push(ChatMessage {
            role,
            content: message.content.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "system",
        content: reminder,
    });

    let options = Options {
        temperature,
        min_p: 0.05,
        top_p: 1.0,
        repeat_penalty: 1.08,
        repeat_last_n: 256,
        num_ctx: CONTEXT_TOKENS,
        num_predict: style.num_predict,
        stop: vec![format!("\n{USER_NAME}:"), format!("{USER_NAME}:")],
    };
    Prompt { messages, options }
}

/// Phase 1 cleanup: drops a leading name prefix, and trims a reply that hit the
/// token cap back to its last complete sentence.
pub fn clean_reply(text: &str, character_name: &str, cut_at_cap: bool) -> String {
    let mut text = text.trim();
    let first_name = character_name
        .split_whitespace()
        .next()
        .unwrap_or(character_name);
    for prefix in [character_name, first_name] {
        if let Some(rest) = text.strip_prefix(prefix).and_then(|r| r.strip_prefix(':')) {
            text = rest.trim_start();
            break;
        }
    }
    if cut_at_cap
        && let Some((at, ch)) = text
            .char_indices()
            .rev()
            .find(|(_, c)| matches!(c, '.' | '!' | '?' | '"' | '*' | '\u{201D}'))
    {
        return text[..at + ch.len_utf8()].trim_end().to_owned();
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ren() -> Character {
        Character {
            id: 1,
            name: "Ren Kagami".into(),
            avatar_path: None,
            tagline: String::new(),
            personality: "Tired.".into(),
            scenario: "{{user}} comes to his office.".into(),
            greeting: "Sit.".into(),
            example_dialogue: "{{user}}: Hi\n{{char}}: Talk.".into(),
            voice_anchor: "Short, dry sentences.".into(),
            default_tone: "dark".into(),
            default_reply_style: "conversational".into(),
            model_override: None,
            age: 34,
        }
    }

    fn chat() -> Chat {
        Chat {
            id: 1,
            character_id: 1,
            title: String::new(),
            tone: "dark".into(),
            reply_style: "conversational".into(),
            chat_model: None,
            head_message_id: None,
        }
    }

    fn msg(role: &str, content: &str) -> Message {
        Message {
            id: 0,
            chat_id: 1,
            parent_id: None,
            role: role.into(),
            content: content.into(),
            model: None,
            created_at: 0,
        }
    }

    #[test]
    fn order_is_rules_card_history_reminder() {
        let p = build(
            &ren(),
            &chat(),
            &[msg("character", "Sit."), msg("user", "Hello")],
        );
        let roles: Vec<&str> = p.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, ["system", "assistant", "user", "system"]);
        let system = &p.messages[0].content;
        assert!(system.contains("You are Ren Kagami"));
        assert!(system.contains("User comes to his office."));
        assert!(system.contains("User: Hi\nRen Kagami: Talk."));
        assert!(!system.contains("{{"));
        let reminder = &p.messages[3].content;
        assert!(reminder.contains("Short, dry sentences."));
        assert!(reminder.ends_with("Reply as Ren Kagami only."));
        assert_eq!(p.options.num_predict, 250);
    }

    #[test]
    fn oldest_messages_drop_first() {
        let long = "word ".repeat(4000);
        let history = [
            msg("user", &long),
            msg("user", &long),
            msg("user", "newest"),
        ];
        let p = build(&ren(), &chat(), &history);
        assert_eq!(p.messages.len(), 4, "one long message and the newest fit");
        assert_eq!(p.messages[2].content, "newest");
    }

    #[test]
    fn cleanup() {
        assert_eq!(
            clean_reply("Ren: Sit down.", "Ren Kagami", false),
            "Sit down."
        );
        assert_eq!(clean_reply("Ren Kagami: Sit.", "Ren Kagami", false), "Sit.");
        assert_eq!(clean_reply("Sit. Then we tal", "Ren Kagami", true), "Sit.");
        assert_eq!(clean_reply("no end", "Ren Kagami", true), "no end");
    }
}
