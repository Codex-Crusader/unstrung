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
    /// The first sentence describes the weather or the room, not the character.
    pub setting_opener: bool,
    /// Words inside quotation marks.
    pub dialogue_words: usize,
    /// Narration outside quotes has a sentence where the user's persona acts,
    /// for example "Kai takes the glass".
    pub narrates_user: bool,
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

/// Words that open a "weather and room" sentence.
const SETTING_WORDS: [&str; 22] = [
    "storm", "rain", "thunder", "lightning", "wind", "candle", "candlelight", "fire",
    "fireplace", "flame", "flames", "shadows", "room", "air", "night", "manor", "house",
    "walls", "window", "sky", "silence", "fog",
];

/// True when the first sentence is about the setting: it has a setting word
/// in its first five words, and it has no dialogue and no name of the character.
pub fn setting_opener(text: &str, character: &str) -> bool {
    let first = text
        .trim_start_matches(['*', '_', ' ', '\n'])
        .split(['.', '!', '?', '\n'])
        .next()
        .unwrap_or_default();
    let first_name = character.split_whitespace().next().unwrap_or(character);
    if first.contains(['"', '\u{201c}']) || has_word(first, first_name) {
        return false;
    }
    first
        .split_whitespace()
        .take(5)
        .any(|w| SETTING_WORDS.iter().any(|s| has_word(w, s)))
}

/// True when a sentence outside quotes starts with the persona's name and a verb-like word.
pub fn narrates_user(text: &str, persona: &str) -> bool {
    // Drop quoted speech, which can name the user freely.
    let mut narration = String::new();
    let mut inside = false;
    for ch in text.chars() {
        match ch {
            '"' => inside = !inside,
            '\u{201c}' => inside = true,
            '\u{201d}' => inside = false,
            _ if !inside => narration.push(ch),
            _ => {}
        }
    }
    narration
        .split(['.', '!', '?', '*', '\n', ';'])
        .any(|sentence| guard::narrates_user(sentence, persona))
}

/// Spoken words. With quotes, the words inside them. With `*actions*` and no
/// quotes, the words outside the asterisks.
pub fn dialogue_words(text: &str) -> usize {
    let quoted = text.contains(['"', '\u{201c}']);
    if !quoted && text.contains('*') {
        return text
            .split('*')
            .step_by(2)
            .map(|part| part.split_whitespace().count())
            .sum();
    }
    let mut inside = false;
    let mut count = 0;
    let mut word = false;
    for ch in text.chars() {
        match ch {
            '"' => inside = !inside,
            '\u{201c}' => inside = true,
            '\u{201d}' => inside = false,
            _ => {}
        }
        let letter = ch.is_alphanumeric();
        if inside && letter && !word {
            count += 1;
        }
        word = letter;
    }
    count
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
        setting_opener: setting_opener(text, cx.character),
        dialogue_words: dialogue_words(text),
        narrates_user: narrates_user(text, cx.persona),
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
    fn setting_and_dialogue() {
        assert!(setting_opener("The storm roars outside, the thunder echoing.", "Ash"));
        assert!(setting_opener("*The candlelight casts eerie shadows.* Hi.", "Ash"));
        assert!(!setting_opener("*Ash sets a candle down.* The storm is loud.", "Ash"));
        assert!(!setting_opener("\"Stay,\" Ash says as the rain falls.", "Ash"));
        assert_eq!(dialogue_words("He said \"stay here now\" and left. \u{201c}Go.\u{201d}"), 4);
        assert_eq!(dialogue_words("*Ash nods.* Stay here. *He waits.* Please."), 3);
        assert!(narrates_user("Ash pours. Kai takes the glass, smiling.", "Kai"));
        assert!(narrates_user("*Kai's hands shake.*", "Kai"));
        assert!(!narrates_user("\"Kai takes too long,\" Ash says.", "Kai"));
        assert!(!narrates_user("Ash looks at Kai. Kai, sit.", "Kai"));
    }

    #[test]
    fn clean_reply_passes() {
        let text = "Door was unlocked. *Ren leans back.* Sit, kid.";
        assert_eq!(
            check(text, text, &[], &CX),
            Flags {
                words: 8,
                dialogue_words: 5,
                ..Flags::default()
            }
        );
    }
}
