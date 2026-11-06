//! The adult content setting and the one gate that every reply and every
//! picture reads again. Nothing here takes the setting from the page: the
//! core reads it from the `meta` table each time.
//!
//! The hard limit is not a setting: sexual content with anyone under 18 is
//! never allowed. The prompt rule, `pictures::safety` and the ages in the
//! database carry it whether the setting is on or off.

use crate::db::{Character, CharacterFields, Db, Persona};
use crate::error::Result;
use crate::library::ADULT_AGE;

/// `meta` key: "on" after the user turned the setting on and confirmed
/// they are 18 or older. Anything else means off.
pub const ADULT_KEY: &str = "adult_content";

pub async fn adult_on(db: &Db) -> Result<bool> {
    Ok(db.meta(ADULT_KEY).await?.as_deref() == Some("on"))
}

pub async fn set_adult(db: &Db, on: bool) -> Result<()> {
    db.set_meta(ADULT_KEY, if on { "on" } else { "off" }).await
}

/// Everyone in the chat is a known adult: an 18+ character with an adult
/// age, and a persona with an adult age. No persona, or a persona without
/// an age, is not enough. The age is required, not only the flag, so an
/// old or imported row with the flag and a young age unlocks nothing.
pub fn adult_people(character: &Character, persona: Option<&Persona>) -> bool {
    character.is_adult
        && character.age >= ADULT_AGE
        && persona
            .and_then(|p| p.age)
            .is_some_and(|age| age >= ADULT_AGE)
        && profile_minor_word(character, persona).is_none()
}

/// The first word in the card or the persona that points to someone under
/// 18. Such a card or persona keeps the chat general, also when the ages
/// say 18 or older: an age field is easy to set, and the text says who the
/// model plays.
pub fn profile_minor_word(character: &Character, persona: Option<&Persona>) -> Option<String> {
    let card = [
        &character.name,
        &character.tagline,
        &character.personality,
        &character.scenario,
        &character.greeting,
        &character.example_dialogue,
        &character.voice_anchor,
        &character.appearance_tags,
        &character.outfit,
    ];
    let persona_text = persona.map_or(Vec::new(), |p| {
        vec![&p.name, &p.description, &p.appearance_tags, &p.outfit]
    });
    first_minor_word(card.into_iter().chain(persona_text))
}

/// The same check for a card that the user saves or imports.
pub fn card_minor_word(f: &CharacterFields) -> Option<String> {
    first_minor_word([
        &f.name,
        &f.tagline,
        &f.personality,
        &f.scenario,
        &f.greeting,
        &f.example_dialogue,
        &f.voice_anchor,
        &f.appearance_tags,
        &f.outfit,
    ])
}

fn first_minor_word<'a>(texts: impl IntoIterator<Item = &'a String>) -> Option<String> {
    texts
        .into_iter()
        .find_map(|t| crate::pictures::safety::minor_word(t))
}

/// Mature content is allowed in this chat now. Read per reply and per
/// picture, because the persona can change in the chat header.
pub async fn mature_chat(
    db: &Db,
    character: &Character,
    persona: Option<&Persona>,
) -> Result<bool> {
    Ok(adult_people(character, persona) && adult_on(db).await?)
}

/// The chat screen asks for the persona's age once: the setting is on,
/// the character is 18+, and the persona has no age. A chat without a
/// persona stays general.
pub fn needs_persona_age(adult_on: bool, character: &Character, persona: Option<&Persona>) -> bool {
    adult_on
        && character.is_adult
        && character.age >= ADULT_AGE
        && persona.is_some_and(|p| p.age.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn people(is_adult: bool, age: i64, persona_age: Option<i64>) -> (Character, Persona) {
        let mut c = crate::prompt::tests::ren();
        c.is_adult = is_adult;
        c.age = age;
        let mut p = crate::prompt::tests::kai();
        p.age = persona_age;
        (c, p)
    }

    #[test]
    fn each_condition_turns_mature_off() {
        let (c, p) = people(true, 30, Some(25));
        assert!(adult_people(&c, Some(&p)));
        for (is_adult, age, persona_age) in [
            (false, 30, Some(25)),
            (true, 17, Some(25)),
            (true, 30, Some(17)),
            (true, 30, None),
        ] {
            let (c, p) = people(is_adult, age, persona_age);
            assert!(
                !adult_people(&c, Some(&p)),
                "{is_adult} {age} {persona_age:?}"
            );
        }
        assert!(!adult_people(&c, None), "no persona");
    }

    #[test]
    fn card_or_persona_text_about_a_minor_turns_mature_off() {
        let (mut c, p) = people(true, 30, Some(25));
        c.scenario = "She is a schoolgirl.".into();
        assert!(!adult_people(&c, Some(&p)));
        assert_eq!(
            profile_minor_word(&c, Some(&p)).as_deref(),
            Some("schoolgirl")
        );
        let (c, mut p) = people(true, 30, Some(25));
        p.description = "A fifteen-year-old runaway.".into();
        assert!(!adult_people(&c, Some(&p)));
        let fields = CharacterFields {
            appearance_tags: "loli, maid".into(),
            ..CharacterFields::default()
        };
        assert_eq!(card_minor_word(&fields).as_deref(), Some("loli"));
    }

    #[test]
    fn a_persona_without_an_age_is_asked() {
        let (c, p) = people(true, 30, None);
        assert!(needs_persona_age(true, &c, Some(&p)));
        assert!(!needs_persona_age(false, &c, Some(&p)), "setting off");
        assert!(!needs_persona_age(true, &c, None), "no persona");
        let (young, p) = people(true, 17, None);
        assert!(!needs_persona_age(true, &young, Some(&p)));
        let (plain, p) = people(false, 30, None);
        assert!(!needs_persona_age(true, &plain, Some(&p)));
    }

    #[tokio::test]
    async fn the_gate_reads_the_setting_from_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db")).await.unwrap();
        let (c, p) = people(true, 30, Some(25));
        assert!(
            !mature_chat(&db, &c, Some(&p)).await.unwrap(),
            "off at install"
        );
        set_adult(&db, true).await.unwrap();
        assert!(mature_chat(&db, &c, Some(&p)).await.unwrap());
        db.set_meta(ADULT_KEY, "yes").await.unwrap();
        assert!(
            !mature_chat(&db, &c, Some(&p)).await.unwrap(),
            "only \"on\" counts"
        );
    }
}
