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
    "[SYSTEM_PROMPT]",
    "[/SYSTEM_PROMPT]",
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
/// 3. removes lines where the persona speaks or acts (see `cut_at_user_line`),
/// 4. trims a reply cut by the token cap to its last full sentence, and closes `*` and quotes,
/// 5. collapses runs of empty lines.
///
/// `continued` is true when `text` starts with an earlier reply, which the
/// user may have edited. Then the first line always stays.
pub fn clean(
    text: &str,
    char_name: &str,
    user_name: &str,
    cut_at_cap: bool,
    continued: bool,
) -> String {
    let mut text = strip_template_tokens(text);
    // A reply that copies the end reminder stops before it.
    if let Some(at) = text.find(GUIDE_MARK) {
        text.truncate(at);
    }
    text = strip_name_prefix(text.trim(), char_name).to_owned();
    text = strip_labels(&text);
    text = cut_at_user_line(&text, user_name, continued);
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
    let others: Vec<Vec<String>> = recent.iter().rev().take(3).map(|e| words_of(e)).collect();
    // The start of a reply gets the strict rule, the same as `StartWatch`.
    // Further in, one sentence said again is not worth a new reply, so only
    // a long copied run counts.
    let start = &words[..words.len().min(START_WORDS)];
    let copied = others.iter().any(|o| shares_run(&words, o, LONG_RUN));
    (repeats(start, &others) || copied).then_some(Problem::Repetition)
}

/// A run of words that counts as a copy anywhere in a reply.
const LONG_RUN: usize = 20;

/// True when `words` start like one of `others`, or share a run of 10 words
/// with one of them.
fn repeats(words: &[String], others: &[Vec<String>]) -> bool {
    others.iter().any(|other| {
        (words.len() >= 5 && other.len() >= 5 && words[..5] == other[..5])
            || shares_run(words, other, 10)
    })
}

/// Words at the start of a reply that wait for the repeat check.
const START_WORDS: usize = 20;

/// Watches the start of a streaming reply for a copy of a recent reply, with
/// the same rule as `check`. The first words wait until the check clears, so
/// a copied opening never reaches the screen, and the caller can stop and
/// write the reply again before the user sees anything.
pub struct StartWatch {
    others: Vec<Vec<String>>,
    held: String,
    open: bool,
    pub repeated: bool,
}

impl StartWatch {
    /// With no `recent` replies, all text passes at once.
    pub fn new(recent: &[&str]) -> Self {
        Self {
            others: recent.iter().take(3).map(|e| words_of(e)).collect(),
            held: String::new(),
            open: recent.is_empty(),
            repeated: false,
        }
    }

    /// Adds streamed text and returns the text to show now.
    pub fn push(&mut self, piece: &str) -> String {
        if self.open {
            return piece.to_owned();
        }
        if self.repeated {
            return String::new();
        }
        self.held.push_str(piece);
        let words = words_of(&self.held);
        let ended = self.held.ends_with(|c: char| !c.is_alphanumeric() && c != '\'');
        let complete = if ended { words.len() } else { words.len().saturating_sub(1) };
        if repeats(&words[..complete], &self.others) {
            self.repeated = true;
            return String::new();
        }
        if complete >= START_WORDS {
            self.open = true;
            return std::mem::take(&mut self.held);
        }
        String::new()
    }

    /// The held-back start, when the stream ends before it is long enough.
    pub fn finish(&mut self) -> String {
        if self.open || self.repeated {
            return String::new();
        }
        if repeats(&words_of(&self.held), &self.others) {
            self.repeated = true;
            return String::new();
        }
        std::mem::take(&mut self.held)
    }
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

/// Words that a model copies from the reply guide as a label, as in
/// "New fact: my father kept old newspapers".
const LABEL_NOUNS: [&str; 4] = ["thing", "fact", "detail", "event"];

/// Removes guide labels such as "New fact:" or "One new thing:" from a reply.
fn strip_labels(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(colon) = rest.find(':') {
        let head = &rest[..colon];
        // The label is the last few words before the colon.
        let start = head
            .char_indices()
            .rev()
            .find(|&(_, c)| !(c.is_alphabetic() || c == ' ' || c == '\''))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let words: Vec<String> = head[start..].split_whitespace().map(str::to_lowercase).collect();
        let at = words.iter().position(|w| w == "new");
        let label = at.is_some_and(|n| {
            words.get(n + 1).is_some_and(|w| LABEL_NOUNS.contains(&w.as_str()))
                && n <= 1
                && (words.len() == n + 2 || words.get(n + 2).is_some_and(|w| w == "about"))
                && words.len() <= n + 5
        });
        if label {
            out.push_str(head[..start].trim_end_matches(' '));
            if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            rest = rest[colon + 1..].trim_start_matches(' ');
        } else {
            out.push_str(&rest[..=colon]);
            rest = &rest[colon + 1..];
        }
    }
    out.push_str(rest);
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
/// True when a sentence starts with the persona acting ("Kai nods") or with
/// the persona's body or things ("Kai's hands shake"). The quality check
/// uses it on every sentence, the guard on line starts.
pub fn narrates_user(sentence: &str, persona: &str) -> bool {
    let mut words = sentence.trim_start().trim_start_matches('*').split_whitespace();
    let (Some(first), Some(second)) = (words.next(), words.next()) else {
        return false;
    };
    let first = first.replace('\u{2019}', "'");
    let second = second.trim_matches(|c: char| !c.is_alphabetic());
    (first == persona
        && second.ends_with('s')
        && second.len() > 2
        && second.starts_with(|c: char| c.is_lowercase()))
        || first == format!("{persona}'s")
}

/// True when a line is the user's part: "Kai: ...", "*Kai ..." or a line
/// that narrates Kai.
fn is_user_line(line: &str, user_name: &str) -> bool {
    let t = line.trim_start();
    !user_name.trim().is_empty()
        && (t.starts_with(&format!("{user_name}:"))
            || t.starts_with(&format!("*{user_name} "))
            || narrates_user(t, user_name))
}

/// Watches a reply while it streams, with the same rule as `cut_at_user_line`.
/// The start of each line waits until its first words show whose line it is,
/// so a line that the guard cuts never reaches the screen. When the model
/// starts to write the user's part after some text, `cut` turns true and the
/// caller stops the stream.
pub struct LiveCut<'a> {
    user_name: &'a str,
    text: String,
    shown: usize,
    line_start: usize,
    decided: bool,
    skip: bool,
    kept_any: bool,
    pub cut: bool,
}

impl<'a> LiveCut<'a> {
    pub fn new(user_name: &'a str) -> Self {
        Self {
            user_name,
            text: String::new(),
            shown: 0,
            line_start: 0,
            decided: false,
            skip: false,
            kept_any: false,
            cut: false,
        }
    }

    /// Adds streamed text and returns the text to show now.
    pub fn push(&mut self, piece: &str) -> String {
        let mut out = String::new();
        if self.cut {
            return out;
        }
        self.text.push_str(piece);
        loop {
            let rest = &self.text[self.line_start..];
            let end = rest.find('\n');
            if !self.decided {
                let head = end.map_or(rest, |e| &rest[..e]);
                if end.is_none() && !self.knows_owner(head) {
                    break;
                }
                self.decide(head.to_owned());
                if self.cut {
                    return out;
                }
            }
            let upto = end.map_or(self.text.len(), |e| self.line_start + e + 1);
            self.show(upto, &mut out);
            match end {
                Some(_) => {
                    self.line_start = upto;
                    self.decided = false;
                    self.skip = false;
                }
                None => break,
            }
        }
        out
    }

    /// The held-back end of the reply, when the stream is over.
    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        if self.cut {
            return out;
        }
        if !self.decided {
            let head = self.text[self.line_start..].to_owned();
            self.decide(head);
            if self.cut {
                return out;
            }
        }
        self.show(self.text.len(), &mut out);
        out
    }

    /// A line shows its owner when it has the user's name and one more
    /// word, or when it is long enough that it cannot be a short name line.
    fn knows_owner(&self, head: &str) -> bool {
        let head = head.trim_start();
        let mut words = head.split_whitespace().count();
        if !head.ends_with(char::is_whitespace) {
            words = words.saturating_sub(1);
        }
        words > self.user_name.split_whitespace().count() || head.len() >= 80
    }

    fn decide(&mut self, head: String) {
        self.decided = true;
        if is_user_line(&head, self.user_name) {
            if self.kept_any {
                self.cut = true;
            } else {
                self.skip = true;
            }
        }
    }

    fn show(&mut self, upto: usize, out: &mut String) {
        let piece = &self.text[self.shown..upto];
        if !self.skip {
            self.kept_any |= !piece.trim().is_empty();
            out.push_str(piece);
        }
        self.shown = upto;
    }
}

fn cut_at_user_line(text: &str, user_name: &str, keep_first: bool) -> String {
    if user_name.trim().is_empty() {
        return text.to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if !(keep_first && i == 0) && is_user_line(line, user_name) {
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
        let c = |t: &str| clean(t, "Ren Kagami", "Kai", false, false);
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
        // A continued reply keeps its first line, which the user may have written.
        assert_eq!(
            clean("*Kai sits.* Ren nods.\nKai: Hi.", "Ren", "Kai", false, true),
            "*Kai sits.* Ren nods."
        );
        // The character may name the user.
        assert_eq!(c("Fine.\nKai, sit down."), "Fine.\nKai, sit down.");
        assert_eq!(c("*He nods"), "*He nods*");
        assert_eq!(c("*"), "");
        assert_eq!(c("Sit.\n\n[Reply guide for Ren. Not part"), "Sit.");
        assert_eq!(c("a\n\n\n\nb"), "a\n\nb");
        // Labels copied from the reply guide go.
        assert_eq!(c("*Ren nods.* Sure. New fact: My father kept old papers."), "*Ren nods.* Sure. My father kept old papers.");
        assert_eq!(c("Fine.\nOne new thing: the lamp flickers."), "Fine.\nthe lamp flickers.");
        assert_eq!(c("New detail about Ren: he limps."), "he limps.");
        // Other colons stay.
        assert_eq!(c("Two things: rain and debt. It is 11:30."), "Two things: rain and debt. It is 11:30.");
        assert_eq!(c("\"The new thing is: nobody pays.\""), "\"The new thing is: nobody pays.\"");
        assert_eq!(clean("Sit. Then we tal", "Ren", "Kai", true, false), "Sit.");
        assert_eq!(clean("“Sit.” Then", "Ren", "Kai", true, false), "“Sit.”");
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
        // Later in a long reply, one sentence said again is fine, but a
        // copied paragraph is not.
        let filler = "she checks the gauges and the hull groans around them both ".repeat(3);
        let order = "stay here and keep an eye on the engine she orders";
        let earlier = format!("Mara nods. {order}.");
        assert_eq!(check(&format!("{filler} {order}."), &[&earlier]), None);
        let paragraph = format!("{order} and do not touch the red valve until I come back with the torch");
        assert_eq!(
            check(&format!("{filler} {paragraph}."), &[&format!("Mara nods. {paragraph}.")]),
            Some(Problem::Repetition)
        );
    }

    #[test]
    fn cuts_lines_that_narrate_the_user() {
        let reply = "*Ren leans back.* What makes you think I can do better?\n\n\
                     User nods, pulling out a file. \"The guy's name is Akira Sato.\"\n\n\
                     Ren raises an eyebrow.";
        assert_eq!(cut_at_user_line(reply, "User", false), "*Ren leans back.* What makes you think I can do better?");
        assert!(narrates_user("Kai's hands shake.", "Kai"));
        assert!(narrates_user("*Kai takes the glass.*", "Kai"));
        assert!(!narrates_user("\"Kai takes too long,\" Ash says.", "Kai"));
        assert!(!narrates_user("Kai, sit down.", "Kai"));
        assert!(!narrates_user("Kai Tanaka was my partner.", "Kai"));
    }

    /// Streams `pieces` through a `LiveCut` and returns what the screen got.
    fn stream(pieces: &[&str]) -> (String, bool) {
        let mut live = LiveCut::new("Kai");
        let mut shown = String::new();
        for p in pieces {
            shown.push_str(&live.push(p));
            if live.cut {
                return (shown, true);
            }
        }
        shown.push_str(&live.finish());
        (shown, false)
    }

    #[test]
    fn start_watch_holds_a_copied_opening() {
        let greeting = "Welcome aboard. The engine is dead, the coffee is worse, and you are the first good news I have had all week.";
        let feed = |watch: &mut StartWatch, text: &str| {
            let mut shown = String::new();
            for word in text.split_inclusive(' ') {
                shown.push_str(&watch.push(word));
                if watch.repeated {
                    break;
                }
            }
            shown + &watch.finish()
        };
        // A copy of the greeting never shows.
        let mut watch = StartWatch::new(&[greeting]);
        assert_eq!(feed(&mut watch, "Welcome aboard. The engine is dead, and more."), "");
        assert!(watch.repeated);
        // A copy that starts with other words is found by the 10-word run.
        let mut watch = StartWatch::new(&[greeting]);
        feed(&mut watch, "Welcome aboard, Commander Stone. The engine is dead, the coffee is worse, and you are the first");
        assert!(watch.repeated);
        // A new reply shows in full, after its first 20 words.
        let mut watch = StartWatch::new(&[greeting]);
        let fresh = "Mara wipes her hands on a rag and points at the coupling. That part cracked two jumps ago, and nobody here can weld. Can you?";
        let mut shown = String::new();
        let words: Vec<&str> = fresh.split_inclusive(' ').collect();
        for (i, w) in words.iter().enumerate() {
            shown.push_str(&watch.push(w));
            if i < 19 {
                assert_eq!(shown, "", "word {i}");
            }
        }
        assert_eq!(shown + &watch.finish(), fresh);
        assert!(!watch.repeated);
        // A short reply shows when the stream ends.
        let mut watch = StartWatch::new(&[greeting]);
        assert_eq!(feed(&mut watch, "Fine. Sit."), "Fine. Sit.");
        // With no recent replies, nothing waits.
        let mut watch = StartWatch::new(&[]);
        assert_eq!(watch.push("Welcome"), "Welcome");
    }

    #[test]
    fn live_cut_stops_before_the_user_line() {
        // The user's line never shows, not even its first word.
        let (shown, cut) = stream(&["Ren sits.", "\n\nKai", "'s jaw", " tightened.", "\nRen waits."]);
        assert_eq!((shown.as_str(), cut), ("Ren sits.\n\n", true));
        let (shown, cut) = stream(&["Ren sits.\n", "Kai: ", "Thanks.\n"]);
        assert_eq!((shown.as_str(), cut), ("Ren sits.\n", true));
        // Lines of the character stream as they come.
        let (shown, cut) = stream(&["Ren", " sits", " down.", "\nKai, he", " says, sit."]);
        assert_eq!((shown.as_str(), cut), ("Ren sits down.\nKai, he says, sit.", false));
        // At the start, the guard drops the user's line and keeps the rest.
        let (shown, cut) = stream(&["*Kai nods.*\n", "Ren lights a match."]);
        assert_eq!((shown.as_str(), cut), ("Ren lights a match.", false));
        // A short last line still shows when the stream ends.
        assert_eq!(stream(&["Ren sits.\n", "Go."]).0, "Ren sits.\nGo.");
        // The screen gets the same text that the guard keeps.
        let raw = "*Ren leans back.* Talk.\n\nKai nods, pulling out a file.\n\nRen reads it.";
        let pieces: Vec<String> = raw.chars().map(String::from).collect();
        let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
        assert_eq!(stream(&refs).0.trim(), cut_at_user_line(raw, "Kai", false));
    }
}
