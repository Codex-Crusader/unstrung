//! The reply guide: the lines at the end of the prompt, which small models
//! follow best.
//!
//! `Turn` describes this turn once, from the history and the chat settings.
//! `RULES` turns it into lines, in a fixed order. Each rule names the failure
//! that it prevents, from "Known failures and fixes" in the design, and the
//! tests in `prompt/tests.rs` cover each one. A new line goes into `RULES`
//! with its failure and a test, not into the builder.

use crate::db::Message;

/// A user message with this many words or fewer gives the character little to answer.
const SHORT_MESSAGE_WORDS: usize = 4;
/// Words of the last reply that the guide quotes, so the next one starts differently.
const OPENING_WORDS: usize = 5;

/// The failures that the guide prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The voice fades into a polite assistant in a long chat.
    GenericVoice,
    /// Four paragraphs in Texting mode.
    IgnoresStyle,
    /// The character card's own mood wins, so every tone sounds the same.
    FlatTone,
    /// The same opening words in every reply.
    Repeats,
    /// A request from a few turns ago is gone after some small talk.
    DropsRequest,
    /// A short user message gets only agreement, and the talk stops.
    Passive,
    /// Every reply ends with a question, and the chat feels like an interview.
    Interview,
    /// After a persona change, the character keeps the old name.
    WrongName,
    /// A model needs one more line to behave (from the catalog).
    ModelQuirk,
    /// The reply before broke a check, and the guard writes it again.
    FailedCheck,
    /// The character writes the user's words or actions.
    SpeaksForUser,
}

/// What the guide knows about this turn.
pub struct Turn<'a> {
    pub anchor: &'a str,
    pub push_character: bool,
    pub tone: String,
    pub style: String,
    /// The first words of the last reply, but not of the greeting.
    pub opening: Option<String>,
    /// The character may add things: not a continuation, and not Follow me.
    pub leads: bool,
    /// The user's last messages in a row that are short, up to 2.
    pub short: usize,
    pub ended_with_question: bool,
    pub open_thread: Option<&'a str>,
    /// The conversation move and the easy question for this reply. They rotate.
    pub idea: &'static str,
    pub ask: &'static str,
    pub persona: bool,
    pub model_extra: &'a str,
    pub regenerate_note: Option<&'a str>,
}

pub(super) struct Rule {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fixes: Failure,
    line: fn(&Turn<'_>) -> Option<String>,
}

/// The guide lines, in order.
pub(super) const RULES: [Rule; 11] = [
    Rule { fixes: Failure::GenericVoice, line: voice },
    Rule { fixes: Failure::IgnoresStyle, line: style },
    // After the long style line, so that the tone is nearer the end.
    Rule { fixes: Failure::FlatTone, line: tone },
    Rule { fixes: Failure::Repeats, line: new_opening },
    Rule { fixes: Failure::DropsRequest, line: open_thread },
    Rule { fixes: Failure::Passive, line: keep_talk_going },
    Rule { fixes: Failure::Interview, line: vary_ending },
    Rule { fixes: Failure::WrongName, line: persona_name },
    Rule { fixes: Failure::ModelQuirk, line: model_extra },
    Rule { fixes: Failure::FailedCheck, line: regenerate_note },
    Rule { fixes: Failure::SpeaksForUser, line: user_turn },
];

/// The guide for `turn`, with `{{char}}` and `{{user}}` still in it.
pub fn reminder(head: &str, turn: &Turn<'_>) -> String {
    let lines: Vec<String> = RULES.iter().filter_map(|r| (r.line)(turn)).collect();
    format!("{head}\n{}", lines.join("\n"))
}

fn voice(t: &Turn<'_>) -> Option<String> {
    let anchor = t.anchor.trim();
    if anchor.is_empty() {
        return None;
    }
    let push = if t.push_character { "\nStay true to this voice in every line." } else { "" };
    Some(format!("Voice of {{{{char}}}}: {anchor}{push}"))
}

/// The tone sets the mood, and the user's character card stays true. A card
/// can say anything, so this line names what the tone never changes.
fn tone(t: &Turn<'_>) -> Option<String> {
    Some(format!(
        "Tone: {}\nThe facts in {{{{char}}}}'s description stay true: {{{{char}}}}'s past, \
         relationships and limits. The tone changes the mood of the scene and how {{{{char}}}} \
         feels in it, shown in {{{{char}}}}'s own voice: a grim character can be funny in a grim \
         way, and a warm one can be shaken.",
        t.tone
    ))
}

fn style(t: &Turn<'_>) -> Option<String> {
    Some(format!("Reply style: {}", t.style))
}

fn new_opening(t: &Turn<'_>) -> Option<String> {
    let opening = t.opening.as_ref()?;
    Some(format!(
        "Your last reply began with \"{opening}\". Begin this one with something new that {{{{char}}}} does or says."
    ))
}

fn open_thread(t: &Turn<'_>) -> Option<String> {
    let thread = t.open_thread.filter(|_| t.leads)?;
    Some(format!(
        "Still open from earlier in the story: {thread}. Answer {{{{user}}}}'s new message first. \
         Then, if it fits, {{{{char}}}} comes back to it."
    ))
}

/// A short message gives the character little, so the reply brings one idea
/// and ends with an easy question that gives the user something to say. A
/// full message gets no idea: small models then answer the idea and ignore
/// the message.
fn keep_talk_going(t: &Turn<'_>) -> Option<String> {
    if !t.leads {
        return None;
    }
    let (idea, ask) = (t.idea, t.ask);
    match t.short {
        0 => None,
        // With an open thread, coming back to it is the move.
        1 if t.open_thread.is_some() => Some(format!(
            "{{{{user}}}}'s message is short. End the reply by asking {{{{user}}}} {ask}."
        )),
        1 => Some(format!(
            "{{{{user}}}}'s message is short, so do not only agree. In this reply, {idea} \
             Then end the reply by asking {{{{user}}}} {ask}."
        )),
        _ => Some(format!(
            "The talk is slowing down. {{{{char}}}} now brings in something new and concrete: \
             an event, a person, a discovery or a secret of {{{{char}}}}'s. Then end the reply \
             by asking {{{{user}}}} {ask}."
        )),
    }
}

/// Small models end almost every reply with a question. This makes the
/// endings alternate, except after a short message, where the question is
/// the point.
fn vary_ending(t: &Turn<'_>) -> Option<String> {
    (t.leads && t.short == 0 && t.ended_with_question).then(|| {
        "Your last reply ended with a question. End this one on an action or a statement.".to_owned()
    })
}

/// After a persona change, the history is full of the old name.
fn persona_name(t: &Turn<'_>) -> Option<String> {
    t.persona.then(|| "The person you talk to is named {{user}}.".to_owned())
}

fn model_extra(t: &Turn<'_>) -> Option<String> {
    (!t.model_extra.is_empty()).then(|| t.model_extra.to_owned())
}

fn regenerate_note(t: &Turn<'_>) -> Option<String> {
    t.regenerate_note.map(str::to_owned)
}

fn user_turn(_: &Turn<'_>) -> Option<String> {
    Some(
        "Do not write what {{user}} says, does or feels. Stop when it is {{user}}'s turn.\n\
         Reply as {{char}} only."
            .to_owned(),
    )
}

/// Ways to keep a conversation going. The guide offers one per reply, in
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

/// Easy questions that give a quiet user something to talk about. They
/// rotate, so the chat does not turn into the same question again.
const USER_QUESTIONS: [&str; 5] = [
    "which of two things {{user}} would choose",
    "what {{user}} thinks about something that {{char}} just said or did",
    "about a small memory from {{user}}'s own life that fits the talk",
    "what {{user}} would do in {{char}}'s place",
    "about {{user}}'s day, or why {{user}} came here",
];

fn replies(history: &[Message]) -> usize {
    history.iter().filter(|m| m.role == "character").count()
}

/// The easy question for this reply.
pub(super) fn user_question(history: &[Message]) -> &'static str {
    USER_QUESTIONS[replies(history) % USER_QUESTIONS.len()]
}

/// The move for this reply. A short message has no word worth asking about,
/// so it skips the first move.
pub(super) fn next_move(history: &[Message], short: bool) -> &'static str {
    let moves = if short { &MOVES[1..] } else { &MOVES[..] };
    moves[replies(history) % moves.len()]
}

/// How many of the user's last messages in a row are short, up to 2.
pub(super) fn short_user_turns(history: &[Message]) -> usize {
    history
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .take(2)
        .take_while(|m| m.content.split_whitespace().count() <= SHORT_MESSAGE_WORDS)
        .count()
}

/// True when the character's last reply ended with a question.
pub(super) fn last_ended_with_question(history: &[Message]) -> bool {
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
pub(super) fn last_opening(history: &[Message]) -> Option<String> {
    let at = history.iter().rposition(|m| m.role == "character")?;
    // The greeting is not quoted: on the first reply, small models copy the
    // quoted greeting instead of starting with something new.
    if !history[..at].iter().any(|m| m.role == "user") {
        return None;
    }
    let words: Vec<&str> = history[at]
        .content
        .split_whitespace()
        .take(OPENING_WORDS)
        .collect();
    (!words.is_empty()).then(|| words.join(" ").replace('"', "'"))
}
