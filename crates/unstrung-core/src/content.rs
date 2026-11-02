//! The adult content setting and the one gate that every reply and every
//! picture reads again. Nothing here takes the setting from the page: the
//! core reads it from the `meta` table each time.
//!
//! The hard limit is not a setting: sexual content with anyone under 18 is
//! never allowed. The prompt rule, `pictures::safety` and the ages in the
//! database carry it whether the setting is on or off.

use crate::db::{Character, Db, Persona};
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
