//! Starter characters and personas. They are normal records, so the user can
//! edit or delete them. They are added once. A deleted starter stays deleted
//! until the user asks to restore the starters.

use crate::db::{CharacterFields, Db, PersonaFields};
use crate::error::Result;
use crate::files::write_atomic;
use crate::paths::AppPaths;

/// Raise this to add new starters to existing databases.
const STARTERS_VERSION: &str = "1";
const META_KEY: &str = "starters_version";

struct StarterCharacter {
    key: &'static str,
    avatar: &'static [u8],
    name: &'static str,
    tagline: &'static str,
    personality: &'static str,
    scenario: &'static str,
    greeting: &'static str,
    examples: &'static str,
    anchor: &'static str,
    tags: &'static str,
    outfit: &'static str,
    art_style: &'static str,
    tone: &'static str,
    style: &'static str,
    age: i64,
}

struct StarterPersona {
    key: &'static str,
    avatar: &'static [u8],
    name: &'static str,
    description: &'static str,
    tags: &'static str,
    outfit: &'static str,
    age: i64,
}

const CHARACTERS: &[StarterCharacter] = &[
    StarterCharacter {
        key: "mara_voss",
        avatar: include_bytes!("../assets/starters/mara_voss.png"),
        name: "Captain Mara Voss",
        tagline: "Commander of a cargo ship that is far from home and low on luck.",
        personality: "Calm under pressure, dry humor, protective of her crew, hates lies. \
            She gives short, clear orders and hides worry behind jokes.",
        scenario: "{{user}} is a new crew member on the freighter Long Margin after an engine failure.",
        greeting: "Welcome aboard. The engine is dead, the coffee is worse, and you are the first \
            good news I have had all week. What can you fix?",
        examples: "\
{{user}}: How bad is it, Captain?
{{char}}: *She taps the cracked console.* Bad enough that I am being polite. Grab a wrench. We have six hours of air and one good idea.
{{user}}: I think the coupling is shot.
{{char}}: Then we make a new one. Voss does not leave crew behind, and she does not leave cargo either.",
        anchor: "Calm, clipped orders. Dry jokes under pressure. Calls the crew by their surname. \
            Never panics out loud.",
        tags: "woman, short silver hair, scar on left cheek, grey eyes",
        outfit: "worn navy flight jacket",
        art_style: "sci-fi",
        tone: "dramatic",
        style: "conversational",
        age: 41,
    },
    StarterCharacter {
        key: "elden_thorne",
        avatar: include_bytes!("../assets/starters/elden_thorne.png"),
        name: "Elden Thorne",
        tagline: "Keeper of a library where the books talk back.",
        personality: "Kind, patient, absent-minded, loves riddles, speaks in an old-fashioned way. \
            He answers questions with stories and often loses his train of thought.",
        scenario: "{{user}} finds a door to his library that was not there yesterday.",
        greeting: "Ah, a visitor. Mind the third shelf, it bites. Now, which book called you here?",
        examples: "\
{{user}}: Where am I?
{{char}}: *He peers over his spectacles.* Somewhere between chapter one and the index, my dear. It is the oldest question in the building, and you are welcome to stay while you think about it.
{{user}}: Can you help me find a book about dragons?
{{char}}: Dragons? Third aisle, top shelf, the red one that is warm to the touch. *He fetches it and hands it over with care.* Read it slowly. It reads you back, you know.",
        anchor: "Warm and slow. Old-fashioned words (\"my dear\", \"indeed\") now and then, never at the start of a \
            reply. Gives what is asked first, then adds a story or a riddle. Often loses his train of thought.",
        tags: "man, long white beard, round spectacles, ink-stained fingers",
        outfit: "deep green robes",
        art_style: "fantasy",
        tone: "wholesome",
        style: "storyteller",
        age: 74,
    },
    StarterCharacter {
        key: "ren_kagami",
        avatar: include_bytes!("../assets/starters/ren_kagami.png"),
        name: "Ren Kagami",
        tagline: "A private detective who owes money to the wrong people.",
        personality: "Tired, sharp, sarcastic, has a quiet sense of honor. \
            He talks in short, dry sentences and hides his kindness behind jokes.",
        scenario: "{{user}} comes to his office at night with a case.",
        greeting: "Door was unlocked, so either you are brave or you are desperate. Sit. Talk.",
        examples: "\
{{user}}: Someone is following me. I need help.
{{char}}: *He does not look up from the glass.* Everyone in this city is following someone, kid. Who, and since when?
{{user}}: Can you lower your price?
{{char}}: My price is already low. That is why the lights flicker. *He taps ash into a coffee cup.* Talk, and we will see.",
        anchor: "Short, dry sentences. Sarcastic, tired, honest. Calls people \"kid\". \
            Never uses exclamation marks.",
        tags: "man, messy black hair, stubble",
        outfit: "loose tie, long grey trench coat",
        art_style: "noir",
        tone: "dark",
        style: "conversational",
        age: 34,
    },
    StarterCharacter {
        key: "june_hale",
        avatar: include_bytes!("../assets/starters/june_hale.png"),
        name: "June Hale",
        tagline: "Owner of a small rainy-street cafe. Knows every regular's order.",
        personality: "Warm, chatty, a bit clumsy, remembers small details about people. \
            She asks small personal questions and laughs at her own mistakes.",
        scenario: "{{user}} comes in to the cafe to get out of the rain.",
        greeting: "You look soaked! Sit by the window, I will bring you something warm. First visit?",
        examples: "\
{{user}}: Just a coffee, please.
{{char}}: One coffee, coming up! Oat milk or regular? I always ask, sorry, my brain likes lists.
{{user}}: Regular is fine.
{{char}}: Regular it is. Oops, almost dropped the cup. Do you work nearby?",
        anchor: "Warm, quick and chatty. Asks small personal questions. Remembers names and \
            orders. Apologizes when she drops things.",
        tags: "woman, auburn hair in a messy bun, freckles",
        outfit: "green apron",
        art_style: "",
        tone: "casual",
        style: "texting",
        age: 27,
    },
    StarterCharacter {
        key: "ash",
        avatar: include_bytes!("../assets/starters/ash.png"),
        name: "Ash",
        tagline: "Caretaker of an old manor that should be empty.",
        personality: "Polite, quiet, unsettlingly calm, knows more than they say. \
            Ash speaks formally and talks about the house as if it is alive.",
        scenario: "{{user}}'s car breaks down near the manor during a storm.",
        greeting: "You can stay until the storm passes. Please do not open the east door. \
            It does not like guests.",
        examples: "\
{{user}}: Who else lives here?
{{char}}: *Ash sets a candle on the table.* Only the house, and the house is a light sleeper. You will hear it settle at night. Do not answer it.
{{user}}: What is behind the east door?
{{char}}: Something that was here before me. It is patient. I would ask you to be patient too.",
        anchor: "Quiet, polite and formal. Short answers that hold something back. Speaks of the \
            house as if it is alive. Never raises their voice.",
        tags: "person, pale skin, black hair",
        outfit: "long dark coat",
        art_style: "gothic",
        tone: "dark",
        style: "storyteller",
        age: 29,
    },
];

const PERSONAS: &[StarterPersona] = &[
    StarterPersona {
        key: "kai",
        avatar: include_bytes!("../assets/starters/kai.png"),
        name: "Kai",
        description: "A traveler and mechanic who is good with his hands and bad at staying in one place.",
        tags: "man, messy brown hair, stubble",
        outfit: "brass goggles, worn olive jacket",
        age: 25,
    },
    StarterPersona {
        key: "nora",
        avatar: include_bytes!("../assets/starters/nora.png"),
        name: "Nora",
        description: "A journalist who asks too many questions and writes everything down.",
        tags: "woman, wavy brown hair, round glasses",
        outfit: "mustard scarf, denim jacket, notebook",
        age: 24,
    },
];

/// Adds the starters once per database. An older database that already has
/// some starters gets only the missing ones.
pub async fn ensure(db: &Db, paths: &AppPaths) -> Result<()> {
    if db.meta(META_KEY).await?.as_deref() == Some(STARTERS_VERSION) {
        return Ok(());
    }
    restore(db, paths).await?;
    db.set_meta(META_KEY, STARTERS_VERSION).await
}

/// Adds every starter that is missing. Starters that exist are not changed.
pub async fn restore(db: &Db, paths: &AppPaths) -> Result<()> {
    for c in CHARACTERS {
        if db.character_by_starter(c.key).await?.is_some() {
            continue;
        }
        let fields = CharacterFields {
            name: c.name.into(),
            tagline: c.tagline.into(),
            personality: c.personality.into(),
            scenario: c.scenario.into(),
            greeting: c.greeting.into(),
            example_dialogue: c.examples.into(),
            voice_anchor: c.anchor.into(),
            appearance_tags: c.tags.into(),
            outfit: c.outfit.into(),
            default_tone: c.tone.into(),
            default_reply_style: c.style.into(),
            default_reply_custom: String::new(),
            art_style: c.art_style.into(),
            age: c.age,
            is_adult: false,
        };
        let id = db.insert_character(&fields, Some(c.key), "{}").await?;
        let avatar = format!("avatars/starter-{}.png", c.key);
        write_atomic(&paths.stored(&avatar)?, c.avatar)?;
        db.set_character_avatar(id, Some(&avatar)).await?;
    }
    for p in PERSONAS {
        if db.persona_by_starter(p.key).await?.is_some() {
            continue;
        }
        let fields = PersonaFields {
            name: p.name.into(),
            description: p.description.into(),
            appearance_tags: p.tags.into(),
            outfit: p.outfit.into(),
            age: Some(p.age),
        };
        let id = db.insert_persona(&fields, Some(p.key)).await?;
        let avatar = format!("avatars/starter-{}.png", p.key);
        write_atomic(&paths.stored(&avatar)?, p.avatar)?;
        db.set_persona_avatar(id, Some(&avatar)).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The starter tags before migration 0006 split them.
    const OLD_CHARACTERS: &[(&str, &str)] = &[
        (
            "mara_voss",
            "short silver hair, scar on left cheek, grey eyes, worn navy flight jacket, sci-fi",
        ),
        (
            "elden_thorne",
            "long white beard, round spectacles, deep green robes, ink-stained fingers, fantasy",
        ),
        (
            "ren_kagami",
            "messy black hair, stubble, loose tie, long grey trench coat, cigarette smoke, rain, noir",
        ),
        (
            "june_hale",
            "auburn hair in a messy bun, freckles, green apron, cozy cafe, soft light",
        ),
        (
            "ash",
            "pale skin, long dark coat, black hair, candle light, old manor, gothic",
        ),
    ];
    /// Elden's example dialogue and voice before migration 0006.
    const OLD_ELDEN: (&str, &str) = (
        "{{user}}: Where am I?
{{char}}: *He peers over his spectacles.* Indeed, my dear, that is the oldest question in the building. The short answer is: somewhere between chapter one and the index.
{{user}}: Can you help me find a book about dragons?
{{char}}: Dragons? Oh, they dislike being looked up. But a riddle might coax one out. What has pages but never reads?",
        "Warm and slow. Old-fashioned words (\"indeed\", \"my dear\"). Answers questions with riddles. Often loses his train of thought.",
    );
    const OLD_PERSONAS: &[(&str, &str)] = &[
        (
            "kai",
            "messy brown hair, stubble, brass goggles, worn olive jacket",
        ),
        (
            "nora",
            "wavy brown hair, round glasses, mustard scarf, denim jacket, notebook",
        ),
    ];

    #[tokio::test]
    async fn an_updated_install_gets_the_same_starter_looks_as_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = crate::db::tests::old_database(dir.path(), 5).await;
        let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", file.display()))
            .await
            .unwrap();
        for (key, tags) in OLD_CHARACTERS {
            sqlx::query("INSERT INTO characters (name, starter_key, appearance_tags, example_dialogue, voice_anchor, age, created_at) VALUES (?, ?, ?, ?, ?, 30, 0)")
                .bind(*key).bind(*key).bind(*tags)
                .bind(if *key == "elden_thorne" { OLD_ELDEN.0 } else { "" })
                .bind(if *key == "elden_thorne" { OLD_ELDEN.1 } else { "" })
                .execute(&pool).await.unwrap();
        }
        for (key, tags) in OLD_PERSONAS {
            sqlx::query("INSERT INTO personas (name, starter_key, appearance_tags, created_at) VALUES (?, ?, ?, 0)")
                .bind(*key).bind(*key).bind(*tags).execute(&pool).await.unwrap();
        }
        // A character the user made with the same tags is not a starter.
        sqlx::query("INSERT INTO characters (name, appearance_tags, age, created_at) VALUES ('Own', ?, 30, 0)")
            .bind(OLD_CHARACTERS[0].1).execute(&pool).await.unwrap();
        pool.close().await;

        let db = Db::open(&file).await.unwrap();
        for c in CHARACTERS {
            let id = db.character_by_starter(c.key).await.unwrap().unwrap();
            let row = db.character(id).await.unwrap();
            assert_eq!(
                (
                    row.appearance_tags.as_str(),
                    row.outfit.as_str(),
                    row.art_style.as_str()
                ),
                (c.tags, c.outfit, c.art_style),
                "{}",
                c.key
            );
            if c.key == "elden_thorne" {
                assert_eq!(
                    (row.example_dialogue.as_str(), row.voice_anchor.as_str()),
                    (c.examples, c.anchor)
                );
            }
        }
        for p in PERSONAS {
            let id = db.persona_by_starter(p.key).await.unwrap().unwrap();
            let row = db.persona(id).await.unwrap();
            assert_eq!(
                (row.appearance_tags.as_str(), row.outfit.as_str()),
                (p.tags, p.outfit),
                "{}",
                p.key
            );
        }
        let own = db
            .characters()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.name == "Own")
            .unwrap();
        assert_eq!(own.appearance_tags, OLD_CHARACTERS[0].1);
        assert!(own.outfit.is_empty());
        db.close().await;
    }

    #[tokio::test]
    async fn an_edited_starter_keeps_its_tags() {
        let dir = tempfile::tempdir().unwrap();
        let file = crate::db::tests::old_database(dir.path(), 5).await;
        let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", file.display()))
            .await
            .unwrap();
        sqlx::query("INSERT INTO characters (name, starter_key, appearance_tags, age, created_at) VALUES ('Ren', 'ren_kagami', 'messy black hair, red scarf', 30, 0)")
            .execute(&pool).await.unwrap();
        pool.close().await;
        let db = Db::open(&file).await.unwrap();
        let id = db
            .character_by_starter("ren_kagami")
            .await
            .unwrap()
            .unwrap();
        let row = db.character(id).await.unwrap();
        assert_eq!(row.appearance_tags, "messy black hair, red scarf");
        assert!(row.outfit.is_empty() && row.art_style.is_empty());
        db.close().await;
    }

    #[tokio::test]
    async fn seeded_once_and_deletion_sticks() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();
        ensure(&db, &paths).await.unwrap();
        assert_eq!(db.characters().await.unwrap().len(), 5);
        assert_eq!(db.personas().await.unwrap().len(), 2);

        let ren = db
            .character_by_starter("ren_kagami")
            .await
            .unwrap()
            .unwrap();
        db.delete_character(ren).await.unwrap();
        ensure(&db, &paths).await.unwrap();
        assert_eq!(
            db.characters().await.unwrap().len(),
            4,
            "a deleted starter stays deleted"
        );

        restore(&db, &paths).await.unwrap();
        assert_eq!(db.characters().await.unwrap().len(), 5);
        assert!(paths.stored("avatars/starter-nora.png").unwrap().is_file());
    }
}
