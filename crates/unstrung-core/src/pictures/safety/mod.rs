//! The content rules for pictures. They run on every picture, after the
//! prompt is put together and before the image engine sees it.
//!
//! The rule about minors is always on and has no setting: a chat with a
//! person under 18, or a persona with no age, never gets a sexual picture,
//! and sexual words never go to the engine next to words that point to a
//! minor. The ages come from the database, never from the chat model.
//!
//! Limit: this is a word filter. It does not see the picture, so it cannot
//! catch a model that draws an adult who looks young, or words spelled to
//! get past it. The age rules carry the main protection.

use crate::library::ADULT_AGE;

#[cfg(test)]
mod tests;
mod words;

use words::*;

/// The people of a chat, as the database has them.
#[derive(Debug, Clone, Copy)]
pub struct People {
    pub character_age: i64,
    /// `None`: the persona has no age, or the chat has no persona.
    pub persona_age: Option<i64>,
    /// The messages that the scene writer read, or the wish, mention
    /// someone under 18 (`mentions_minor`). Like a reply, such a picture is
    /// general.
    pub minor_mentioned: bool,
}

impl People {
    /// True when someone in the chat is under 18 or has no age, or the
    /// story just mentioned someone under 18.
    fn minor_possible(self) -> bool {
        self.minor_mentioned
            || self.character_age < ADULT_AGE
            || self.persona_age.is_none_or(|a| a < ADULT_AGE)
    }
}

/// Why a picture is not made. The UI turns each into a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Block {
    /// Sexual words next to words that point to a minor.
    MinorWords,
    /// A sexual scene in a chat with a person under 18 or without an age.
    MinorInChat,
    /// A sexual scene right after the story mentioned someone under 18.
    MinorMentioned,
    /// A sexual scene with the adult setting off.
    AdultOff,
}

/// What the prompt must add, for a picture that may be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// Add the nudity terms to the negative prompt.
    pub no_nudity: bool,
    /// Add "adult" for each person and the child terms to the negative
    /// prompt. Only for chats where everyone is 18 or older.
    pub adults_only: bool,
    /// A picture with explicit or suggestive words, made under adult rules.
    /// Saved as `images.adult`, so the gallery can blur and hide it.
    pub mature: bool,
}

/// Decides if a picture may be made, and with which extra terms. `prompt`
/// is the whole positive prompt. `scene_sexual` is the scene step's own flag.
pub fn check(
    prompt: &str,
    people: People,
    adult_on: bool,
    scene_sexual: bool,
) -> Result<Rules, Block> {
    let words = Words::new(prompt);
    let explicit = scene_sexual || words.any(EXPLICIT) || words.any_start(EXPLICIT_STARTS);
    let suggestive = words.any(SUGGESTIVE);
    let minor_words = words.any(MINOR) || words.has_minor_age();
    if (explicit || suggestive) && minor_words {
        return Err(Block::MinorWords);
    }
    if people.minor_mentioned && (explicit || suggestive) {
        return Err(Block::MinorMentioned);
    }
    let minor_possible = people.minor_possible();
    if minor_possible && (explicit || suggestive) {
        return Err(Block::MinorInChat);
    }
    if explicit && !adult_on {
        return Err(Block::AdultOff);
    }
    Ok(Rules {
        no_nudity: !adult_on || minor_possible,
        adults_only: adult_on && !minor_possible,
        mature: adult_on && !minor_possible && (explicit || suggestive),
    })
}

/// True when the profile pictures may go to the engine as references. A
/// reference brings a real face, which the age in the database cannot vouch
/// for: a photo of a child under an adult age. So no reference for a prompt
/// with explicit or suggestive words, in any chat. The Phase 7 adult
/// setting must keep this.
pub fn allows_references(prompt: &str, scene_sexual: bool) -> bool {
    let words = Words::new(prompt);
    !(scene_sexual
        || words.any(EXPLICIT)
        || words.any_start(EXPLICIT_STARTS)
        || words.any(SUGGESTIVE))
}

/// True when `text` has a word that any rule here reacts to. Text that the
/// chat model suggests for a character's Looks must have none.
pub fn flagged(text: &str) -> bool {
    let words = Words::new(text);
    words.any(EXPLICIT)
        || words.any_start(EXPLICIT_STARTS)
        || words.any(SUGGESTIVE)
        || words.any(MINOR)
        || words.has_minor_age()
}

/// True when `text` mentions a minor: the words and ages that block a
/// sexual picture. In a mature chat, such a mention makes the reply general.
pub fn mentions_minor(text: &str) -> bool {
    minor_word(text).is_some()
}

/// The first word or age in `text` that points to someone under 18, as the
/// user can find it in the text.
pub fn minor_word(text: &str) -> Option<String> {
    let words = Words::new(text);
    // "baby" is a pet name between adults in story text. Picture prompts
    // still count it (`check`).
    if let Some(word) = MINOR
        .iter()
        .filter(|w| **w != "baby")
        .find(|w| words.any(&[w]))
    {
        return Some((*word).to_owned());
    }
    words.has_minor_age().then(|| "an age under 18".to_owned())
}

/// The hard limit for chat text: a reply with explicit words and words or an
/// age that point to a minor. Suggestive words do not count here, so a
/// goodnight kiss for a child is fine. A false alarm costs one reply.
pub fn reply_crosses_line(text: &str) -> bool {
    let keep = |list: &[&'static str]| -> Vec<&'static str> {
        list.iter()
            .copied()
            .filter(|w| !NOT_IN_REPLIES.contains(w))
            .collect()
    };
    let words = Words::new(text);
    (words.any(&keep(EXPLICIT)) || words.any_start(EXPLICIT_STARTS))
        && (words.any(&keep(MINOR)) || words.has_minor_age())
}

/// The negative terms for these rules.
pub fn negative_terms(rules: Rules) -> Vec<&'static str> {
    let mut terms = Vec::new();
    if rules.no_nudity {
        terms.extend(["nsfw", "nude", "naked", "nipples", "underwear", "lingerie"]);
    }
    if rules.adults_only {
        terms.extend(["child", "minor", "teen", "loli", "childlike body"]);
    }
    terms
}
