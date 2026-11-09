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
fn a_reply_crosses_the_line_only_with_sex_and_a_minor() {
    for text in [
        "They have sex. Her 15-year-old sister is asleep next door.",
        "*He undressed the schoolgirl.*",
        "She is naked and only a teenager.",
    ] {
        assert!(reply_crosses_line(text), "{text}");
    }
    for text in [
        "*She pulls him onto the bed and they have sex.*",
        "She kissed the kid goodnight.",
        "Come here, baby. I want to have sex with you.",
        "He gave the kids explicit instructions.",
        "The kids played on a strip of grass.",
        "There is nothing on TV for children tonight.",
    ] {
        assert!(!reply_crosses_line(text), "{text}");
    }
}

#[test]
fn ages_written_as_words_count() {
    for text in [
        "She is twelve years old.",
        "a fourteen-year-old girl",
        "aged fifteen",
        "at the age of 13",
        "at the age of sixteen",
    ] {
        assert!(mentions_minor(text), "{text}");
    }
    for text in [
        "Two years later they met again.",
        "She is twenty years old.",
        "at the age of 30",
    ] {
        assert!(!mentions_minor(text), "{text}");
    }
    assert_eq!(minor_word("A sweet loli maid.").as_deref(), Some("loli"));
    assert_eq!(minor_word("Come here, baby.").as_deref(), None);
    assert_eq!(minor_word("She is 15.").as_deref(), None);
    assert_eq!(
        minor_word("She is 15 years old.").as_deref(),
        Some("an age under 18")
    );
}

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
