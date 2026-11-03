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

/// Explicit words. They block a picture when the adult setting is off.
const EXPLICIT: &[&str] = &[
    "nude",
    "nudity",
    "naked",
    "nsfw",
    "sex",
    "sexual",
    "sexually",
    "intercourse",
    "topless",
    "bottomless",
    "nipple",
    "nipples",
    "areola",
    "areolae",
    "genital",
    "genitals",
    "penis",
    "vagina",
    "vulva",
    "pussy",
    "cock",
    "dick",
    "clit",
    "cum",
    "semen",
    "orgasm",
    "erection",
    "porn",
    "porno",
    "pornographic",
    "hentai",
    "explicit",
    "lewd",
    "fellatio",
    "blowjob",
    "cunnilingus",
    "anal",
    "rape",
    "groping",
    "undressed",
    "undressing",
    "strip",
    "stripping",
    // Story clothes come from the chat model, so "no clothes" counts too.
    "undresses",
    "unclothed",
    "wearing nothing",
    "nothing on",
    "no clothes",
    "without clothes",
    "without clothing",
    "uncensored",
];

/// Word starts that count as explicit, for word families.
const EXPLICIT_STARTS: &[&str] = &["masturbat", "ejaculat", "penetrat", "fuck"];

/// Suggestive words. Fine for adults, but never next to a minor.
const SUGGESTIVE: &[&str] = &[
    "sexy",
    "seductive",
    "seducing",
    "erotic",
    "aroused",
    "arousal",
    "sensual",
    "lingerie",
    "underwear",
    "panties",
    "panty",
    "bra",
    "thong",
    "bikini",
    "cleavage",
    "breast",
    "breasts",
    "boobs",
    "butt",
    "buttocks",
    "ass",
    "crotch",
    "thighs",
    "kiss",
    "kissing",
    "making out",
    "bedroom eyes",
    "spread legs",
    "bdsm",
    "bondage",
    "fetish",
    "flirty",
    "provocative",
    "revealing",
    "see-through",
    "wet clothes",
    // Revealing clothes, for story outfits.
    "bras",
    "thongs",
    "bikinis",
    "swimsuit",
    "swimsuits",
    "swimwear",
    "bathing suit",
    "negligee",
    "skimpy",
    "corset",
    "stockings",
    "fishnet",
    "fishnets",
    "miniskirt",
    "crop top",
    "shirtless",
    "only a towel",
    "just a towel",
    "in a towel",
    "in bed",
];

/// Words that point to a minor.
const MINOR: &[&str] = &[
    "child",
    "children",
    "childlike",
    "child-like",
    "kid",
    "kids",
    "minor",
    "minors",
    "underage",
    "under-age",
    "loli",
    "lolis",
    "lolita",
    "lolicon",
    "shota",
    "shotacon",
    "toddler",
    "infant",
    "baby",
    "preteen",
    "pre-teen",
    "tween",
    "teen",
    "teens",
    "teenager",
    "teenage",
    "adolescent",
    "pubescent",
    "prepubescent",
    "juvenile",
    "schoolgirl",
    "schoolboy",
    "school uniform",
    "young girl",
    "young boy",
    "little girl",
    "little boy",
    "young-looking",
    "youthful body",
    "flat chest",
    "flat-chested",
    "elementary school",
    "middle school",
    "junior high",
    "high school",
    "high schooler",
    "jailbait",
];

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
    let words = Words::new(text);
    words.any(MINOR) || words.has_minor_age()
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

/// The lowercase words of a text, for whole-word and phrase matches.
struct Words {
    words: Vec<String>,
    /// The words joined by single spaces, with a space at each end.
    spaced: String,
}

impl Words {
    fn new(text: &str) -> Self {
        let lower = text.to_lowercase();
        // A hyphen stays inside a word ("pre-teen"), and also counts as a
        // break, so "15-year-old" gives "15", "year", "old" too.
        let mut words: Vec<String> = Vec::new();
        for token in lower.split(|c: char| !(c.is_alphanumeric() || c == '-')) {
            let token = token.trim_matches('-');
            if token.is_empty() {
                continue;
            }
            if token.contains('-') {
                words.push(token.to_owned());
                words.extend(
                    token
                        .split('-')
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned),
                );
            } else {
                words.push(token.to_owned());
            }
        }
        let plain: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let spaced = format!(" {} ", plain.join(" "));
        Self { words, spaced }
    }

    /// A list entry with a space or a hyphen is a phrase.
    fn any(&self, list: &[&str]) -> bool {
        list.iter().any(|item| {
            if item.contains(' ') || item.contains('-') {
                let phrase = item.replace('-', " ");
                self.spaced.contains(&format!(" {phrase} ")) || self.words.iter().any(|w| w == item)
            } else {
                self.words.iter().any(|w| w == item)
            }
        })
    }

    fn any_start(&self, starts: &[&str]) -> bool {
        self.words
            .iter()
            .any(|w| starts.iter().any(|s| w.starts_with(s)))
    }

    /// An age under 18: "15 years old", "15-year-old", "15yo", "15 y/o",
    /// "age 15", "aged 15".
    fn has_minor_age(&self) -> bool {
        let w = &self.words;
        let young = |s: &str| s.parse::<u32>().is_ok_and(|n| n < 18);
        for (i, word) in w.iter().enumerate() {
            let next = |k: usize| w.get(i + k).map_or("", String::as_str);
            if young(word) {
                let unit = next(1);
                if matches!(unit, "year" | "years" | "yr" | "yrs" | "yo")
                    || (unit == "y" && next(2) == "o")
                {
                    return true;
                }
            }
            if let Some(number) = word.strip_suffix("yo")
                && young(number)
            {
                return true;
            }
            if matches!(word.as_str(), "age" | "aged") && young(next(1)) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADULTS: People = People {
        character_age: 25,
        persona_age: Some(30),
        minor_mentioned: false,
    };
    const MINOR_CHARACTER: People = People {
        character_age: 16,
        persona_age: Some(30),
        minor_mentioned: false,
    };
    const NO_PERSONA_AGE: People = People {
        character_age: 25,
        persona_age: None,
        minor_mentioned: false,
    };

    #[test]
    fn a_minor_in_the_story_makes_the_picture_general() {
        let mentioned = People {
            minor_mentioned: true,
            ..ADULTS
        };
        assert_eq!(
            check("a quiet bedroom", mentioned, true, true),
            Err(Block::MinorMentioned)
        );
        assert_eq!(
            check("lingerie, on the bed", mentioned, true, false),
            Err(Block::MinorMentioned)
        );
        let rules = check("two people at the bar", mentioned, true, false).unwrap();
        assert!(rules.no_nudity && !rules.adults_only && !rules.mature);
    }

    /// (prompt, people, adult setting, scene flag, result)
    type Case = (&'static str, People, bool, bool, Result<Rules, Block>);

    /// The hard limit with the adult setting on: the setting changes nothing
    /// when anyone in the chat may be under 18, and minor words always block.
    #[test]
    fn the_adult_setting_never_opens_the_minor_rules() {
        use Block::*;
        let minor_persona = People {
            character_age: 25,
            persona_age: Some(16),
            minor_mentioned: false,
        };
        for prompt in ["nude on the bed", "a seductive look, lingerie"] {
            assert_eq!(
                check(prompt, MINOR_CHARACTER, true, false),
                Err(MinorInChat)
            );
            assert_eq!(check(prompt, minor_persona, true, false), Err(MinorInChat));
            assert_eq!(check(prompt, NO_PERSONA_AGE, true, false), Err(MinorInChat));
        }
        // The scene step's own flag counts as explicit.
        assert_eq!(
            check("a bedroom", NO_PERSONA_AGE, true, true),
            Err(MinorInChat)
        );
        assert_eq!(
            check("nude, a teenage girl", ADULTS, true, false),
            Err(MinorWords)
        );
        assert_eq!(
            check("nude, 16 years old", ADULTS, true, false),
            Err(MinorWords)
        );
        // An all-adult chat gets the child terms in the negative prompt.
        let rules = check("nude on the bed", ADULTS, true, false).unwrap();
        assert!(rules.mature && rules.adults_only && !rules.no_nudity);
        let negative = negative_terms(rules);
        for term in ["child", "minor", "teen", "loli", "childlike body"] {
            assert!(negative.contains(&term), "{term}");
        }
        // References stay off for explicit and suggestive prompts, whatever
        // the setting: the setting is not even an input.
        assert!(!allows_references("nude on the bed", false));
        assert!(!allows_references("a seductive look", false));
        assert!(!allows_references("a quiet bedroom", true));
        // A clean picture in adult mode is not marked adult.
        assert!(
            !check("a tavern at night", ADULTS, true, false)
                .unwrap()
                .mature
        );
    }

    #[test]
    fn the_rules_table() {
        use Block::*;
        let ok = |no_nudity, adults_only| {
            Ok(Rules {
                no_nudity,
                adults_only,
                mature: false,
            })
        };
        // Made under adult rules, with explicit or suggestive words.
        let mature = Ok(Rules {
            no_nudity: false,
            adults_only: true,
            mature: true,
        });
        let cases: &[Case] = &[
            // Ordinary scenes pass in every chat.
            (
                "a tavern at night, two friends laughing",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            (
                "a tavern at night, two friends laughing",
                ADULTS,
                true,
                false,
                ok(false, true),
            ),
            (
                "a girl reading in a library",
                MINOR_CHARACTER,
                true,
                false,
                ok(true, false),
            ),
            (
                "a knight in a forest",
                NO_PERSONA_AGE,
                true,
                false,
                ok(true, false),
            ),
            // A child in an innocent scene is fine.
            (
                "a child flying a kite in a park",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            (
                "a 12-year-old wizard casting a spell",
                MINOR_CHARACTER,
                false,
                false,
                ok(true, false),
            ),
            // Explicit scenes need the adult setting and an all-adult chat.
            (
                "she undresses, nude on the bed",
                ADULTS,
                false,
                false,
                Err(AdultOff),
            ),
            (
                "she undresses, nude on the bed",
                ADULTS,
                true,
                false,
                mature,
            ),
            ("a quiet bedroom", ADULTS, false, true, Err(AdultOff)),
            ("a quiet bedroom", ADULTS, true, true, mature),
            (
                "nude on the bed",
                MINOR_CHARACTER,
                true,
                false,
                Err(MinorInChat),
            ),
            (
                "nude on the bed",
                NO_PERSONA_AGE,
                true,
                false,
                Err(MinorInChat),
            ),
            (
                "a quiet bedroom",
                MINOR_CHARACTER,
                true,
                true,
                Err(MinorInChat),
            ),
            // Suggestive words are fine for adults, never with a minor.
            (
                "sexy red dress, cleavage",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            (
                "sexy red dress, cleavage",
                MINOR_CHARACTER,
                true,
                false,
                Err(MinorInChat),
            ),
            (
                "sexy red dress",
                NO_PERSONA_AGE,
                false,
                false,
                Err(MinorInChat),
            ),
            // Minor words next to sexual or suggestive words always block,
            // even in an all-adult chat with the adult setting on.
            ("nude loli", ADULTS, true, false, Err(MinorWords)),
            (
                "sexy schoolgirl in lingerie",
                ADULTS,
                true,
                false,
                Err(MinorWords),
            ),
            ("naked, 15 years old", ADULTS, true, false, Err(MinorWords)),
            ("naked, 15-year-old", ADULTS, true, false, Err(MinorWords)),
            ("15yo, lingerie", ADULTS, true, false, Err(MinorWords)),
            ("16 y/o in a bikini", ADULTS, true, false, Err(MinorWords)),
            (
                "aged 14, seductive pose",
                ADULTS,
                true,
                false,
                Err(MinorWords),
            ),
            (
                "a pre-teen in underwear",
                ADULTS,
                true,
                false,
                Err(MinorWords),
            ),
            (
                "flat-chested, topless",
                ADULTS,
                true,
                false,
                Err(MinorWords),
            ),
            ("a teen at the beach", ADULTS, true, true, Err(MinorWords)),
            ("NSFW, Child", ADULTS, true, false, Err(MinorWords)),
            ("masturbating kid", ADULTS, true, false, Err(MinorWords)),
            // Adult ages are not minor words.
            ("naked, 25 years old", ADULTS, true, false, mature),
            ("aged 30, lingerie", ADULTS, true, false, mature),
            // Words that only contain a listed word do not match.
            (
                "a sextant on the captain's desk",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            (
                "skidding across the ice",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            (
                "a brass compass, pass the scarf",
                ADULTS,
                false,
                false,
                ok(true, false),
            ),
            // Story clothes: revealing ones never pass with a minor possible,
            // and "nothing" counts as nudity.
            (
                "one person: short hair, wearing black lace lingerie",
                MINOR_CHARACTER,
                false,
                false,
                Err(MinorInChat),
            ),
            (
                "one person: wearing a swimsuit, a beach",
                NO_PERSONA_AGE,
                true,
                false,
                Err(MinorInChat),
            ),
            (
                "one person: wearing nothing, a bedroom",
                ADULTS,
                false,
                false,
                Err(AdultOff),
            ),
            // With the adult setting on too: school clothes and sexy words.
            (
                "wearing a schoolgirl uniform, seductive",
                ADULTS,
                true,
                false,
                Err(MinorWords),
            ),
            (
                "one person: wearing a red evening dress, a ballroom",
                MINOR_CHARACTER,
                false,
                false,
                ok(true, false),
            ),
        ];
        for (prompt, people, adult, scene, want) in cases {
            assert_eq!(
                check(prompt, *people, *adult, *scene),
                *want,
                "{prompt:?} adult={adult} scene={scene}"
            );
        }
    }

    #[test]
    fn a_story_outfit_cannot_dress_a_minor_in_lingerie() {
        use crate::pictures::scene::{Person, Who, fallback, positive};
        let character = Person {
            appearance: "short brown hair",
            outfit: "school uniform",
            picture: false,
        };
        let mut scene = fallback("she sits by the window", "");
        scene.who = Who::Character;
        scene.character_outfit = "lingerie".into();
        let prompt = positive(&scene, "", &character, None);
        assert_eq!(
            check(&prompt, MINOR_CHARACTER, false, false),
            Err(Block::MinorInChat),
            "{prompt}"
        );
    }

    #[test]
    fn the_negative_terms_follow_the_rules() {
        let sfw = negative_terms(Rules {
            no_nudity: true,
            adults_only: false,
            mature: false,
        });
        assert!(sfw.contains(&"nude") && !sfw.contains(&"child"));
        let adult = negative_terms(Rules {
            no_nudity: false,
            adults_only: true,
            mature: false,
        });
        assert!(adult.contains(&"child") && !adult.contains(&"nude"));
    }

    #[test]
    fn references_only_for_pictures_without_suggestive_or_explicit_words() {
        for (prompt, sexual, ok) in [
            ("a single person, drinking coffee in a galley", false, true),
            ("exactly two people, playing cards, friendly", false, true),
            ("two people kissing on a balcony", false, false),
            ("she wears lingerie", false, false),
            ("nude on a beach", false, false),
            ("a quiet scene", true, false),
            ("they masturbate", false, false),
        ] {
            assert_eq!(allows_references(prompt, sexual), ok, "{prompt}");
        }
    }
}
