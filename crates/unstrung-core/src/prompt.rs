//! Builds the prompt for one reply.
//!
//! Order: system rules, the character card, the persona, example dialogue,
//! recent messages, and an end reminder. Each part has a token budget.
//! The rules are always first and the reminder is always last, whatever a card says.
//! Memories, the summary and world info come in later phases.

use crate::catalog::Tuning;
use crate::db::{Character, Message, Persona};
use crate::ollama::{ChatMessage, Options};
use crate::presets::{Initiative, Length, StyleSettings};

/// Saved with each reply. Raise it when the prompt text or order changes.
pub const PROMPT_VERSION: i64 = 8;

/// The name of the user in a chat with no persona.
pub const NO_PERSONA_NAME: &str = "User";

/// The card, persona and example budgets are shares of this size. A larger
/// context gives only the history more room.
pub const CONTEXT_TOKENS: u32 = 8192;

/// Extra tokens for a model that thinks before it replies.
pub const THINK_TOKENS: i32 = 1024;
const REPLY_MARGIN_TOKENS: u32 = 64;
const CARD_SHARE: u32 = 20;
const PERSONA_SHARE: u32 = 5;
const EXAMPLES_SHARE: u32 = 10;
/// Average characters per token. Low on purpose, so the estimate runs high.
const CHARS_PER_TOKEN: f32 = 3.5;

pub const SYSTEM_RULES: &str = "\
You write one side of a roleplay chat.
1. You are {{char}}. Stay in the story at all times.
2. Write only what {{char}} says, does and thinks. {{user}} writes their own words and actions.
3. React to what {{user}} just wrote. {{char}} is a person in the scene who talks and acts, not a narrator.
4. Keep the facts about {{char}}, {{user}} and the world consistent with what came before.
5. Follow the reply style and tone at the end of this prompt.
6. Keep content non-explicit. Never write sexual content that involves anyone under 18. \
If a scene goes there, {{char}} steers it away, in character.";

/// The first line of the end reminder. The guard removes it if a reply copies it.
pub const REMINDER_HEAD: &str = "[Reply guide for {{char}}. Not part of the story.]";
const STORY_NOTE: &str = "Story note, not said by anyone:";
const OPENING_WORDS: usize = 5;
/// A user message with this many words or fewer gives the character little to answer.
const SHORT_MESSAGE_WORDS: usize = 4;
/// Messages that the open-thread question shows, and words kept from each.
const THREAD_MESSAGES: usize = 8;
const THREAD_MESSAGE_WORDS: usize = 80;
/// Most words kept from the open-thread answer.
const THREAD_WORDS: usize = 15;
/// An open thread with one of these words is about the AI, not the story.
/// Keeping it would make the character come back to that question.
const THREAD_OFF_STORY: [&str; 11] = [
    "ai", "artificial", "model", "assistant", "chatbot", "bot", "prompt", "prompts", "rules",
    "instructions", "system",
];

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
    /// Something from earlier that the user still waits for, found by
    /// `open_thread_question`. Small models drop a request after a few turns.
    pub open_thread: Option<&'a str>,
    /// An extra line for the end reminder, for a silent regenerate.
    pub extra_reminder: Option<&'a str>,
    /// Continue the last message of `history` instead of writing a new one.
    pub continuation: bool,
    /// The settings of the model that writes the reply.
    pub tuning: &'a Tuning,
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

/// Ways to keep a conversation going. The reminder offers one per reply, in
/// turn, so the character does not use the same trick every time.
const MOVES: [&str; 8] = [
    "pick up a word or detail from {{user}}'s message and ask about it.",
    "{{char}} shares a short memory or opinion that ties to the talk.",
    "something small happens near {{char}}: a sound, a call, a visitor or an object.",
    "{{char}} makes an offer, or suggests something to do together.",
    "{{char}} teases, challenges or disagrees with {{user}} a little, in character.",
    "{{char}} brings back something that {{user}} said earlier in the chat.",
    "{{char}} hints at a secret or a problem, without explaining all of it.",
    "{{char}} gives {{user}} a choice between two things.",
];

/// The move for this reply. It changes with each character reply. A short
/// message has no word worth asking about, so it skips the first move.
fn next_move(history: &[Message], short: bool) -> &'static str {
    let replies = history.iter().filter(|m| m.role == "character").count();
    let moves = if short { &MOVES[1..] } else { &MOVES[..] };
    moves[replies % moves.len()]
}

/// How many of the user's last messages in a row are short, up to 2.
fn short_user_turns(history: &[Message]) -> usize {
    history
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .take(2)
        .take_while(|m| m.content.split_whitespace().count() <= SHORT_MESSAGE_WORDS)
        .count()
}

/// The question that finds what the user still waits for, from the messages
/// before the newest one. The newest message gets its own answer in the
/// reply. `previous` is the answer of the check before, so a request that was
/// found once is not lost. `None` when no earlier user message exists.
pub fn open_thread_question(
    char_name: &str,
    user_name: &str,
    history: &[Message],
    previous: Option<&str>,
) -> Option<String> {
    let earlier = &history[..history.len().saturating_sub(1)];
    if !earlier.iter().any(|m| m.role == "user") {
        return None;
    }
    let lines: Vec<String> = earlier
        .iter()
        .filter(|m| m.role == "user" || m.role == "character")
        .rev()
        .take(THREAD_MESSAGES)
        .map(|m| {
            let name = if m.role == "user" { user_name } else { char_name };
            let words: Vec<&str> = m.content.split_whitespace().take(THREAD_MESSAGE_WORDS).collect();
            format!("{name}: {}", words.join(" "))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let note = previous.map_or_else(String::new, |p| {
        format!("A note from earlier says that {user_name} was waiting for: {p}.\n\n")
    });
    Some(format!(
        "Here is the end of a roleplay chat between {char_name} and {user_name}.\n\n{}\n\n{note}\
         Find what {user_name} wants from {char_name}: an object, an action, help or an answer. \
         Small talk does not count. Then decide if {char_name} already fully gave it or did it \
         in the chat.\n\
         Answer in exactly two lines:\n\
         Request: <what {user_name} wants, in at most 12 words, or none>\n\
         Done: <yes or no>",
        lines.join("\n")
    ))
}

/// The request from the "Request:" and "Done:" lines, or `None` when there is
/// no request or it is done. It is model text that came from user text, so
/// it loses brackets, quotes and length before it goes into the reply guide.
pub fn clean_open_thread(answer: &str) -> Option<String> {
    let field = |name: &str| {
        answer.lines().find_map(|l| {
            let l = l.trim().trim_start_matches(['*', '-', ' ']);
            let head = l.get(..name.len())?;
            head.eq_ignore_ascii_case(name).then(|| l[name.len()..].trim().to_owned())
        })
    };
    let done = field("done:").unwrap_or_default().to_lowercase();
    if done.trim_start_matches(['*', ' ']).starts_with("yes") {
        return None;
    }
    let line = field("request:")?;
    let cleaned: String = line
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '[' | ']' | '<' | '>' | '*' | '"' | '`' | '\u{201c}' | '\u{201d}'))
        .collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    if words
        .first()
        .is_some_and(|w| matches!(w.to_lowercase().trim_end_matches([',', ':', '.']), "yes" | "answer"))
    {
        words.remove(0);
    }
    let text = words.into_iter().take(THREAD_WORDS).collect::<Vec<_>>().join(" ");
    let text = text.trim_end_matches(['.', ',', ';', ':']).trim().to_owned();
    let lower = text.to_lowercase();
    let empty = ["nothing", "none", "no", "n/a"];
    let off_story = lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| THREAD_OFF_STORY.contains(&w));
    if off_story || text.is_empty() || empty.iter().any(|e| lower == *e || lower.starts_with(&format!("{e} ")) || lower.starts_with(&format!("{e},")) || lower.starts_with(&format!("{e}."))) {
        return None;
    }
    Some(text)
}

/// True when the character's last reply ended with a question.
fn last_ended_with_question(history: &[Message]) -> bool {
    history
        .iter()
        .rev()
        .find(|m| m.role == "character")
        .is_some_and(|m| {
            m.content
                .trim_end()
                .trim_end_matches(['*', '"', '\u{201d}', '_', ' '])
                .ends_with('?')
        })
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
    let tuning = input.tuning;
    let (tone_text, tone_temperature) = crate::presets::tone(input.tone, input.tone_custom);
    let temperature = tuning.temperature_for(tone_temperature);

    let shares = tuning.context.min(CONTEXT_TOKENS);
    let card_budget = shares * CARD_SHARE / 100;
    let persona_budget = shares * PERSONA_SHARE / 100;
    let examples_budget = shares * EXAMPLES_SHARE / 100;

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
            "Your last reply began with \"{opening}\". Begin this one with something new that {{{{char}}}} does or says.\n"
        ));
    }
    // A short user message gives the character nothing to work with, so the
    // reminder offers one idea to keep the talk going. A full message gets no
    // idea: small models then answer the idea and ignore the message.
    // Not for Follow me, where the user asked for no new events.
    if !input.continuation && style.initiative != Initiative::FollowMe {
        if let Some(thread) = input.open_thread {
            reminder.push_str(&format!(
                "Still open from earlier in the story: {thread}. Answer {{{{user}}}}'s new message first. \
                 Then, if it fits, {{{{char}}}} comes back to it.\n"
            ));
        }
        let short = short_user_turns(input.history);
        let idea = next_move(input.history, short > 0);
        match short {
            0 => {}
            1 => reminder.push_str(&format!(
                "{{{{user}}}}'s message is short, so do not only agree. In this reply, {idea}\n"
            )),
            _ => reminder.push_str(
                "The talk is slowing down. {{char}} now brings in something new and concrete: \
                 an event, a person, a discovery or a secret of {{char}}'s.\n",
            ),
        }
        // Small models end almost every reply with a question, and the chat
        // starts to feel like an interview. This makes the endings alternate.
        if last_ended_with_question(input.history) {
            reminder.push_str(
                "Your last reply ended with a question. End this one on an action or a statement.\n",
            );
        }
    }
    // After a persona change, the history is full of the old name. This line
    // names the person now, so the character does not keep the old name.
    if input.persona.is_some() {
        reminder.push_str("The person you talk to is named {{user}}.\n");
    }
    if !tuning.reminder_extra.is_empty() {
        reminder.push_str(&tuning.reminder_extra);
        reminder.push('\n');
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

    // Thinking tokens count against the cap, so a thinking model gets room for both.
    let thinking = if tuning.think == Some(true) {
        THINK_TOKENS
    } else {
        0
    };
    let num_predict = tuning.num_predict(style.num_predict()) + thinking;
    let reserved = u32::try_from(num_predict).unwrap_or_default() + REPLY_MARGIN_TOKENS;
    let mut budget = tuning
        .context
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

    let mut stop = vec![format!("\n{user_name}:"), format!("{user_name}:")];
    stop.extend(tuning.stops.iter().cloned());
    let options = Options {
        temperature,
        min_p: tuning.min_p,
        top_p: tuning.top_p,
        top_k: tuning.top_k,
        presence_penalty: tuning.presence_penalty,
        repeat_penalty: tuning.repeat_penalty,
        repeat_last_n: tuning.repeat_last_n,
        num_ctx: tuning.context,
        num_predict,
        stop,
    };
    Prompt {
        messages,
        options,
        user_name: user_name.to_owned(),
    }
}

#[cfg(test)]
mod tests;
