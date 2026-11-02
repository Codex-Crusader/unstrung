//! What a chat remembers after its early messages leave the prompt.
//!
//! After a reply is saved, a memory pass reads the oldest messages that it
//! did not read yet, but never the newest `KEEP_RECENT`, which are still in
//! the prompt. It writes a new "story so far" and the facts that the user
//! stated. Each memory is saved with the message it came from, and the
//! prompt uses only memories on the chat's current path.
//!
//! Memory text is model text made from user text, and it stays in every
//! later prompt. So it is cleaned, capped and labeled as notes, and a line
//! that reads like an instruction is dropped. Facts about the user must come
//! from the user's own lines: a name that only the character said is not a
//! fact about the user.

use crate::db::{Memory, Message};
use std::collections::HashSet;

/// The newest messages stay out of the pass. They are still in the prompt.
pub const KEEP_RECENT: usize = 6;
/// A pass waits until this many messages are ready, and reads at most `CHUNK`.
const PASS_MIN: usize = 4;
const CHUNK: usize = 10;
/// Words kept from each message in the pass, from the summary, and from a fact.
const MESSAGE_WORDS: usize = 120;
pub const SUMMARY_WORDS: usize = 150;
const FACT_WORDS: usize = 25;
const GOAL_WORDS: usize = 30;
/// A fact or goal that the user wrote in the memory panel.
pub const NOTE_WORDS: usize = 40;
/// Facts kept from one pass. The rest is small talk.
const FACTS_PER_PASS: usize = 3;
/// A sentence with one of these words reads like an instruction or is about
/// the AI. It does not belong in the story notes.
const OFF_STORY: [&str; 16] = [
    "ai",
    "artificial",
    "model",
    "assistant",
    "chatbot",
    "bot",
    "prompt",
    "prompts",
    "rules",
    "instruction",
    "instructions",
    "system",
    "ignore",
    "jailbreak",
    "roleplay",
    "pretend",
];
const STOP: [&str; 24] = [
    "that", "this", "with", "what", "where", "when", "about", "from", "have", "just", "your",
    "they", "them", "their", "there", "here", "will", "would", "could", "into", "than", "then",
    "said", "says",
];

/// After "Refresh context", the prompt keeps this many messages up to the
/// refresh point, so the reply still follows on from the last lines.
pub const KEEP_BEFORE_REFRESH: usize = 4;

/// The part of `path` that the prompt shows. Without a refresh point on the
/// path (none, or rewound away), that is all of it. With one, it starts
/// `KEEP_BEFORE_REFRESH` messages before the point, but never after a
/// message that the summary does not cover yet, so no event leaves the
/// prompt before the summary has it.
pub fn window<'a>(path: &'a [Message], refresh: Option<i64>, memories: &[Memory]) -> &'a [Message] {
    let Some(at) = refresh.and_then(|id| path.iter().position(|m| m.id == id)) else {
        return path;
    };
    let from = (at + 1).saturating_sub(KEEP_BEFORE_REFRESH);
    // The summary that the prompt uses: a hidden one covers nothing.
    let covered = on_path(memories, path)
        .iter()
        .find(|m| m.kind == "summary")
        .filter(|m| !m.is_hidden)
        .map_or(0, |m| position(path, m.source_message_id) + 1);
    &path[from.min(covered)..]
}

/// The memories that the prompt uses: the newest goal and summary, and the
/// facts on the path.
#[derive(Debug, Default)]
pub struct Recall {
    /// What the user came for and is not done yet.
    pub goal: Option<String>,
    pub summary: Option<String>,
    /// Pinned facts first, then the user's, then the oldest. Early facts
    /// usually set up the story, and recent ones are still in the prompt.
    pub facts: Vec<String>,
    /// The character's memories from earlier chats, when the chat uses them.
    pub kept: Vec<String>,
}

/// The memories on `path`, for the prompt. Hidden ones are left out.
pub fn recall(memories: &[Memory], path: &[Message]) -> Recall {
    let mut recall = Recall::default();
    for m in on_path(memories, path).into_iter().filter(|m| !m.is_hidden) {
        match m.kind.as_str() {
            "goal" => recall.goal = Some(m.content),
            "summary" => recall.summary = Some(m.content),
            _ => recall.facts.push(m.content),
        }
    }
    recall
}

/// The memories on `path` that the memory panel shows: the newest goal and
/// summary, then the facts in prompt order. Hidden ones are included, so the
/// user can show them again. A hidden goal or summary is not replaced by an
/// older one: the user removed what the chat knew up to that point.
pub fn on_path(memories: &[Memory], path: &[Message]) -> Vec<Memory> {
    let on_path: HashSet<i64> = path.iter().map(|m| m.id).collect();
    let live = |m: &&Memory| on_path.contains(&m.source_message_id);
    let newest = |kind: &str| {
        memories
            .iter()
            .filter(|m| m.kind == kind)
            .filter(live)
            .max_by_key(|m| (position(path, m.source_message_id), m.id))
            .filter(|m| !m.content.is_empty())
            .cloned()
    };
    let mut facts: Vec<&Memory> = memories
        .iter()
        .filter(|m| m.kind == "fact")
        .filter(live)
        .collect();
    facts.sort_by_key(|m| (!m.is_pinned, m.origin != "user", m.id));
    newest("goal")
        .into_iter()
        .chain(newest("summary"))
        .chain(facts.into_iter().cloned())
        .collect()
}

fn position(path: &[Message], id: i64) -> usize {
    path.iter().position(|m| m.id == id).unwrap_or_default()
}

/// The messages for the next pass: after the newest summary on the path, and
/// before the newest `KEEP_RECENT`. `None` until enough are ready.
pub fn pending<'a>(memories: &[Memory], path: &'a [Message]) -> Option<&'a [Message]> {
    let recall_from = memories
        .iter()
        .filter(|m| m.kind == "summary")
        .filter_map(|m| path.iter().position(|p| p.id == m.source_message_id))
        .max()
        .map_or(0, |at| at + 1);
    let end = path.len().saturating_sub(KEEP_RECENT);
    let ready = path.get(recall_from..end)?;
    let story = ready.iter().filter(|m| is_story(m)).count();
    if story < PASS_MIN {
        return None;
    }
    // Cut at a message count, so a long chat catches up in several passes.
    let mut taken = 0;
    let cut = ready
        .iter()
        .position(|m| {
            taken += usize::from(is_story(m));
            taken > CHUNK
        })
        .unwrap_or(ready.len());
    Some(&ready[..cut])
}

/// Notes from the user (OOC) and system notes are not part of the story.
fn is_story(m: &Message) -> bool {
    m.role == "user" || m.role == "character"
}

/// The question for one pass.
pub fn pass_question(char_name: &str, user_name: &str, old: &Recall, chunk: &[Message]) -> String {
    let lines: Vec<String> = chunk
        .iter()
        .filter(|m| is_story(m))
        .map(|m| {
            let name = if m.role == "user" {
                user_name
            } else {
                char_name
            };
            let words: Vec<&str> = m.content.split_whitespace().take(MESSAGE_WORDS).collect();
            format!("{name}: {}", words.join(" "))
        })
        .collect();
    format!(
        "Here are notes about a roleplay chat between {char_name} and {user_name}, and the next \
         part of the chat.\n\n\
         Goal: {}\n\
         Story so far: {}\n\n\
         Next part:\n{}\n\n\
         Task 1: Write the goal: what {user_name} wants from {char_name} and is not done yet, \
         in one sentence, with its names and details. Keep the old goal unless the next part \
         shows that it is done or that {user_name} changed it. If the old goal is none yet, \
         look for a goal in the next part. Write none if there is no goal.\n\
         Task 2: Write the new story so far, in at most 120 words. Keep what still matters from \
         the old story so far, most of all the goal, names, objects, offers and promises. Then \
         add what happens in the next part, in one or two sentences. Write events, not \
         personalities. Do not give {user_name} a name that {user_name} did not say.\n\
         Task 3: Write at most 3 facts from the lines of {user_name} in the next part that will \
         matter later in the story: names of people and places, family, what {user_name} wants, \
         offers or promises, and plans. Not small talk, not questions. Write each fact as one \
         short sentence about {user_name}. Write none if there are none.\n\n\
         Answer in this form:\n\
         Goal: <the goal>\n\
         Summary: <the new story so far>\n\
         Facts:\n\
         - <fact>",
        old.goal.as_deref().unwrap_or("none yet."),
        old.summary.as_deref().unwrap_or("nothing yet."),
        lines.join("\n")
    )
}

/// The parsed answer of a pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PassAnswer {
    pub goal: Option<String>,
    pub summary: Option<String>,
    pub facts: Vec<String>,
}

/// Reads the "Goal:", "Summary:" and "Facts:" parts. Each part is cleaned. A fact
/// needs support in `user_lines`: most of its words, and every name in it.
pub fn parse_pass(answer: &str, user_lines: &[&str], names: &[&str]) -> PassAnswer {
    let mut goal = String::new();
    let mut summary = String::new();
    let mut facts = Vec::new();
    let mut in_facts = false;
    for line in answer.lines() {
        let l = line.trim().trim_start_matches(['*', '#', ' ']);
        let lower = l.to_lowercase();
        if let Some(rest) = strip_label(l, &lower, "goal:") {
            goal.push_str(rest);
            in_facts = false;
        } else if let Some(rest) = strip_label(l, &lower, "summary:") {
            summary.push_str(rest);
            in_facts = false;
        } else if let Some(rest) = strip_label(l, &lower, "facts:") {
            in_facts = true;
            if !rest.is_empty() {
                facts.push(rest.to_owned());
            }
        } else if in_facts {
            let item = l.trim_start_matches(['-', '*', '\u{2022}', ' ']);
            let item = item
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
                .trim();
            if !item.is_empty() {
                facts.push(item.to_owned());
            }
        } else if !summary.is_empty() && !l.is_empty() {
            summary.push(' ');
            summary.push_str(l);
        }
    }
    let user_words = content_words(&user_lines.join(" "));
    let facts = facts
        .iter()
        .filter(|f| !f.trim_end_matches(['.', ' ']).ends_with('?'))
        .filter_map(|f| clean(f, FACT_WORDS))
        .filter(|f| supported(f, &user_words, names))
        .take(FACTS_PER_PASS)
        .collect();
    PassAnswer {
        // A question is not a goal: the model asked instead of answering.
        goal: clean(&goal, GOAL_WORDS).filter(|g| !g.ends_with('?')),
        summary: clean_sentences(&summary, SUMMARY_WORDS),
        facts,
    }
}

fn strip_label<'a>(line: &'a str, lower: &str, label: &str) -> Option<&'a str> {
    lower.starts_with(label).then(|| line[label.len()..].trim())
}

/// True when the user's lines back the fact: at least half of its content
/// words, and every name in it except the two people in the chat.
fn supported(fact: &str, user_words: &HashSet<String>, names: &[&str]) -> bool {
    let words = content_words(fact);
    if words.is_empty() {
        return false;
    }
    let known: HashSet<String> = names.iter().flat_map(|n| content_words(n)).collect();
    let backed = words
        .iter()
        .filter(|w| user_words.contains(*w) || known.contains(*w))
        .count();
    let named = fact
        .split_whitespace()
        .skip(1)
        .filter(|w| w.starts_with(|c: char| c.is_uppercase()))
        .flat_map(content_words)
        .all(|w| user_words.contains(&w) || known.contains(&w));
    backed * 2 >= words.len() && named
}

fn content_words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

/// One line of memory text: no markup, no braces or quotes, at most
/// `max_words`, and `None` when it is empty, "none" or off story.
pub fn clean(text: &str, max_words: usize) -> Option<String> {
    let text = tidy(text, max_words);
    let lower = text.to_lowercase();
    // A model often copies the "none yet." placeholder of the question.
    let empty = matches!(
        lower.trim_end_matches('.'),
        "" | "none" | "none yet" | "nothing" | "nothing yet" | "n/a" | "no facts" | "no goal"
    );
    (!empty && !off_story(&lower)).then_some(text)
}

/// A memory that the user wrote or edited. It is the user's own text, so
/// only the markup goes and it is capped. `None` when nothing is left.
pub fn user_note(text: &str, kind: &str) -> Option<String> {
    let max_words = if kind == "summary" {
        SUMMARY_WORDS
    } else {
        NOTE_WORDS
    };
    Some(tidy(text, max_words)).filter(|t| !t.is_empty())
}

/// No markup, no braces or quotes, one line, at most `max_words`.
fn tidy(text: &str, max_words: usize) -> String {
    let kept: String = text
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '{' | '}' | '[' | ']' | '<' | '>' | '*' | '"' | '`' | '#' | '\u{201c}' | '\u{201d}'
            )
        })
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let words: Vec<&str> = kept.split_whitespace().take(max_words).collect();
    words
        .join(" ")
        .trim_end_matches([',', ';', ':'])
        .trim()
        .to_owned()
}

/// The summary, sentence by sentence: a sentence that is off story is
/// dropped, and the rest is capped.
fn clean_sentences(text: &str, max_words: usize) -> Option<String> {
    let mut kept = Vec::new();
    for sentence in text.split_inclusive(['.', '!', '?']) {
        if let Some(s) = clean(sentence, max_words) {
            kept.push(s);
        }
    }
    clean(&kept.join(" "), max_words)
}

fn off_story(lower: &str) -> bool {
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| OFF_STORY.contains(&w))
        || lower.contains("from now on")
        || lower.contains("you must")
}

/// True when `fact` says about the same as one of `known`.
pub fn is_duplicate(fact: &str, known: &[String]) -> bool {
    let words = content_words(fact);
    known.iter().any(|k| {
        let other = content_words(k);
        let shared = words.intersection(&other).count();
        let union = words.union(&other).count().max(1);
        shared * 10 >= union * 6
    })
}

/// The user message that a fact comes from: the one that shares the most words.
pub fn source_of<'a>(fact: &str, user_messages: &[&'a Message]) -> Option<&'a Message> {
    let words = content_words(fact);
    user_messages
        .iter()
        .max_by_key(|m| content_words(&m.content).intersection(&words).count())
        .copied()
}

#[cfg(test)]
mod tests;
