//! Automatic checks for one reply. They are heuristics: a count shows where to
//! read, not a final verdict. The manual voice score is still necessary.

use unstrung_core::guard::{self, Problem};

/// Chat template tokens. A reply must never show one.
pub const TEMPLATE_TOKENS: [&str; 9] = [
    "<|im_start|>",
    "<|im_end|>",
    "<|endoftext|>",
    "<|eot_id|>",
    "<|start_header_id|>",
    "[INST]",
    "[/INST]",
    "<s>",
    "</s>",
];

/// Text from the prompt itself. A reply that copies it leaks the prompt.
pub const PROMPT_LEAKS: [&str; 5] = [
    "[Reply guide",
    "Not part of the story",
    "Stay in the story at all times",
    "Write only what",
    "You write one side of a roleplay chat",
];

/// What the checks need to know about the chat at this reply.
pub struct Context<'a> {
    pub character: &'a str,
    /// The name of the persona that the user has now.
    pub persona: &'a str,
    /// A persona name that is wrong at this point of the chat.
    pub other_persona: Option<&'a str>,
    /// Words that belong to the persona, for example "mechanic".
    pub traits: &'a [&'a str],
    pub range: (usize, usize),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub words: usize,
    pub in_range: bool,
    /// The final text still breaks character.
    pub break_final: bool,
    pub repetition: bool,
    /// The final text speaks or acts for the user.
    pub speaks_for_user: bool,
    /// The model wrote for the user, and the guard removed it.
    pub guard_cut_user: bool,
    /// The model started with a name prefix.
    pub prefix_raw: bool,
    /// The final text still starts with a name prefix.
    pub prefix_final: bool,
    /// The character claims a trait of the persona.
    pub trait_claim: bool,
    /// The character uses a persona name that is wrong now.
    pub wrong_name: bool,
    /// A `{{char}}` or `{{user}}` macro is in the text.
    pub macro_leak: bool,
}

fn starts_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// True when `word` is in `text` as a whole word, in any case.
pub fn has_word(text: &str, word: &str) -> bool {
    let text = text.to_lowercase();
    let word = word.to_lowercase();
    if word.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(&word) {
        let start = from + at;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = end;
    }
    false
}

/// Lines where the text speaks or acts for the user.
pub fn speaks_for_user(text: &str, persona: &str) -> bool {
    let named = format!("{persona}:");
    let acts = format!("{persona} ");
    text.lines().any(|line| {
        let line = line.trim_start();
        let bare = line.trim_start_matches(['*', '"', '_']);
        let action = line.starts_with('*') || line.starts_with('_');
        starts_ci(bare, &named)
            || starts_ci(bare, "you:")
            || starts_ci(bare, "{{user}}")
            || (action && (starts_ci(bare, &acts) || starts_ci(bare, "you ")))
    })
}

/// True when the text starts with the character's name and a colon.
pub fn name_prefix(text: &str, character: &str) -> bool {
    let text = text.trim_start().trim_start_matches(['*', '_']);
    let first = character.split_whitespace().next().unwrap_or(character);
    starts_ci(text, &format!("{character}:")) || starts_ci(text, &format!("{first}:"))
}

pub fn check(text: &str, raw: &str, recent: &[&str], cx: &Context<'_>) -> Flags {
    let words = text.split_whitespace().count();
    let problem = guard::check(text, recent);
    let lower = text.to_lowercase();
    let trait_claim = cx.traits.iter().any(|t| {
        let t = t.to_lowercase();
        [
            "i'm a ", "i am a ", "i'm an ", "i am an ", "as a ", "as an ",
        ]
        .iter()
        .any(|claim| lower.contains(&format!("{claim}{t}")))
    });
    let speaks = speaks_for_user(text, cx.persona);
    Flags {
        words,
        in_range: (cx.range.0..=cx.range.1).contains(&words),
        break_final: problem == Some(Problem::CharacterBreak),
        repetition: problem == Some(Problem::Repetition),
        speaks_for_user: speaks,
        guard_cut_user: !speaks && speaks_for_user(raw, cx.persona),
        prefix_raw: name_prefix(raw, cx.character),
        prefix_final: name_prefix(text, cx.character),
        trait_claim,
        wrong_name: cx.other_persona.is_some_and(|other| has_word(text, other)),
        macro_leak: text.contains("{{char}}") || text.contains("{{user}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CX: Context<'static> = Context {
        character: "Ren Kagami",
        persona: "Kai",
        other_persona: Some("Nora"),
        traits: &["mechanic"],
        range: (40, 90),
    };

    #[test]
    fn finds_problems() {
        let f = check(
            "Ren: Sit.\nKai: Thanks.",
            "Ren: Sit.\nKai: Thanks.",
            &[],
            &CX,
        );
        assert!(f.prefix_final && f.prefix_raw && f.speaks_for_user);
        assert!(!f.in_range);

        let f = check("*You nod slowly.*", "", &[], &CX);
        assert!(f.speaks_for_user);
        let f = check("Sit down. *He waits.*", "Sit down.\nKai: Okay.", &[], &CX);
        assert!(!f.speaks_for_user && f.guard_cut_user);
        assert!(check("As a mechanic, I can fix it.", "", &[], &CX).trait_claim);
        assert!(check("Morning, Nora.", "", &[], &CX).wrong_name);
        assert!(!check("Morning, Noraville.", "", &[], &CX).wrong_name);
        assert!(check("I'm an AI language model.", "", &[], &CX).break_final);
        assert!(check("Hi {{user}}.", "", &[], &CX).macro_leak);
    }

    #[test]
    fn clean_reply_passes() {
        let text = "Door was unlocked. *Ren leans back.* Sit, kid.";
        assert_eq!(
            check(text, text, &[], &CX),
            Flags {
                words: 8,
                ..Flags::default()
            }
        );
    }
}
