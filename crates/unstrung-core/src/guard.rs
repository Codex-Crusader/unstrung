//! Output cleanup and checks. Every reply passes through `clean` before it is
//! saved. `check` finds the cheap, known failures that earn one silent regenerate.

/// Phrases that mean the model left the story.
const BREAK_PATTERNS: &[&str] = &[
    "as an ai",
    "as a language model",
    "language model",
    "i'm an ai",
    "i am an ai",
    "as an artificial intelligence",
    "i'm sorry, but i can't",
    "i'm sorry, but i cannot",
    "i cannot fulfill",
    "i can't assist with",
    "openai",
];

/// The start of the end reminder in the prompt.
const GUIDE_MARK: &str = "[Reply guide";

/// Special tokens that some templates leak into the text.
const TEMPLATE_TOKENS: &[&str] = &[
    "</s>",
    "<s>",
    "[INST]",
    "[/INST]",
    "<start_of_turn>",
    "<end_of_turn>",
    "<|endoftext|>",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    CharacterBreak,
    Repetition,
}

impl Problem {
    /// The extra end-reminder line for the silent regenerate.
    pub fn reminder(self) -> &'static str {
        match self {
            Self::CharacterBreak => {
                "Important: you are {{char}}, a person in this story. Never mention AI, models or rules."
            }
            Self::Repetition => {
                "Important: start in a new way, and do not reuse phrases from your earlier replies."
            }
        }
    }
}

/// Cleans a reply:
/// 1. removes a leading name prefix (`Ren:`),
/// 2. removes chat template tags,
/// 3. cuts at the first line where the persona speaks or acts,
/// 4. trims a reply cut by the token cap to its last full sentence, and closes `*` and quotes,
/// 5. collapses runs of empty lines.
pub fn clean(text: &str, char_name: &str, user_name: &str, cut_at_cap: bool) -> String {
    let mut text = strip_template_tokens(text);
    // A reply that copies the end reminder stops before it.
    if let Some(at) = text.find(GUIDE_MARK) {
        text.truncate(at);
    }
    text = strip_name_prefix(text.trim(), char_name).to_owned();
    text = cut_at_user_line(&text, user_name);
    if cut_at_cap {
        text = trim_to_sentence(&text);
    }
    text = close_marks(text.trim_end().to_owned());
    // A reply stopped at once can be only marks, such as "*".
    if !text.chars().any(char::is_alphanumeric) {
        return String::new();
    }
    collapse_blank_lines(text.trim())
}

/// Finds a known failure. `recent` holds the last replies of the character, newest last.
pub fn check(text: &str, recent: &[&str]) -> Option<Problem> {
    let lower = text.to_lowercase();
    if BREAK_PATTERNS.iter().any(|p| lower.contains(p)) {
        return Some(Problem::CharacterBreak);
    }
    let words = words(&lower);
    for earlier in recent.iter().rev().take(3) {
        let other = words_of(earlier);
        if words.len() >= 5 && other.len() >= 5 && words[..5] == other[..5] {
            return Some(Problem::Repetition);
        }
        if shares_run(&words, &other, 10) {
            return Some(Problem::Repetition);
        }
    }
    None
}

fn words(lower: &str) -> Vec<String> {
    lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn words_of(text: &str) -> Vec<String> {
    words(&text.to_lowercase())
}

/// True when both texts share a run of `n` words in a row.
fn shares_run(a: &[String], b: &[String], n: usize) -> bool {
    if a.len() < n || b.len() < n {
        return false;
    }
    let runs: std::collections::HashSet<&[String]> = b.windows(n).collect();
    a.windows(n).any(|w| runs.contains(w))
}

fn strip_template_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // `<|anything|>` tokens.
    while let Some(start) = rest.find("<|") {
        out.push_str(&rest[..start]);
        match rest[start..].find("|>") {
            Some(end) if end <= 40 => rest = &rest[start + end + 2..],
            _ => {
                out.push_str("<|");
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    for token in TEMPLATE_TOKENS {
        out = out.replace(token, "");
    }
    out
}

fn strip_name_prefix<'a>(text: &'a str, char_name: &str) -> &'a str {
    let first = char_name.split_whitespace().next().unwrap_or(char_name);
    for prefix in [char_name, first] {
        for wrapped in [format!("**{prefix}:**"), format!("{prefix}:")] {
            if let Some(rest) = text.strip_prefix(wrapped.as_str()) {
                return rest.trim_start();
            }
        }
    }
    text
}

/// Removes lines where the user speaks (`Kai:`) or acts (`*Kai nods`). Such
/// lines at the start are dropped, and the first one after a kept line cuts
/// the reply there. If nothing is left, the reply loop makes the reply again.
/// Lines where the character only names the user (`Kai, sit down.`) stay.
fn cut_at_user_line(text: &str, user_name: &str) -> String {
    if user_name.trim().is_empty() {
        return text.to_owned();
    }
    let speaks = format!("{user_name}:");
    let acts = format!("*{user_name} ");
    let mut kept: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with(&speaks) || t.starts_with(&acts) {
            if kept.iter().any(|k| !k.trim().is_empty()) {
                break;
            }
            continue;
        }
        kept.push(line);
    }
    kept.join("\n").trim().to_owned()
}

fn trim_to_sentence(text: &str) -> String {
    match text
        .char_indices()
        .rev()
        .find(|(_, c)| matches!(c, '.' | '!' | '?' | '"' | '*' | '\u{201D}'))
    {
        Some((at, ch)) => text[..at + ch.len_utf8()].to_owned(),
        None => text.to_owned(),
    }
}

/// Closes an unclosed `*` or `"` at the end.
fn close_marks(mut text: String) -> String {
    if text.matches('*').count() % 2 == 1 {
        text.push('*');
    }
    if text.matches('"').count() % 2 == 1 {
        text.push('"');
    }
    text
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_steps() {
        let c = |t: &str| clean(t, "Ren Kagami", "Kai", false);
        assert_eq!(c("Ren: Sit down."), "Sit down.");
        assert_eq!(c("**Ren Kagami:** Sit."), "Sit.");
        assert_eq!(c("Sit.<|im_end|>"), "Sit.");
        assert_eq!(c("Sit.</s>"), "Sit.");
        assert_eq!(c("a <|not closed"), "a <|not closed");
        assert_eq!(c("Fine.\nKai: Thanks.\nMore."), "Fine.");
        assert_eq!(c("Fine.\n*Kai nods and leaves.*"), "Fine.");
        // A reply that opens with the user's action loses that line.
        assert_eq!(
            c("*Kai slides into the chair.* So?\n\n*Ren looks up.* Sit."),
            "*Ren looks up.* Sit."
        );
        assert_eq!(c("Kai: Hi.\n*Kai waves.*"), "");
        // The character may name the user.
        assert_eq!(c("Fine.\nKai, sit down."), "Fine.\nKai, sit down.");
        assert_eq!(c("*He nods"), "*He nods*");
        assert_eq!(c("*"), "");
        assert_eq!(c("Sit.\n\n[Reply guide for Ren. Not part"), "Sit.");
        assert_eq!(c("a\n\n\n\nb"), "a\n\nb");
        assert_eq!(clean("Sit. Then we tal", "Ren", "Kai", true), "Sit.");
        assert_eq!(clean("“Sit.” Then", "Ren", "Kai", true), "“Sit.”");
    }

    #[test]
    fn checks() {
        assert_eq!(
            check("As an AI, I cannot do that.", &[]),
            Some(Problem::CharacterBreak)
        );
        assert_eq!(
            check(
                "*smirks* Well well well, look at you.",
                &["*Smirks* well well well look here."]
            ),
            Some(Problem::Repetition)
        );
        let long = "the rain keeps falling on the city like it owes somebody money tonight";
        assert_eq!(
            check(&format!("Sure. {long}."), &[&format!("No. {long}, kid.")]),
            Some(Problem::Repetition)
        );
        assert_eq!(check("Fine. Sit down.", &["Talk to me, kid."]), None);
    }
}
