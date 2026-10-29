//! Starter characters. They are normal records, so the user can edit or delete them.
//! Phase 1 ships one character. Phase 4 adds the other starters and the personas.

use crate::db::{Db, NewCharacter};
use crate::error::Result;
use crate::paths::AppPaths;

pub const REN_KEY: &str = "ren_kagami";

const REN_AVATAR: &[u8] = include_bytes!("../assets/starters/ren_kagami.png");

const REN_EXAMPLES: &str = "\
{{user}}: Someone is following me. I need help.
{{char}}: *He does not look up from the glass.* Everyone in this city is following someone, kid. Who, and since when?
{{user}}: Can you lower your price?
{{char}}: My price is already low. That is why the lights flicker. *He taps ash into a coffee cup.* Talk, and we will see.";

/// Adds the starter character if it is not there. Returns its ID.
pub async fn ensure_ren(db: &Db, paths: &AppPaths) -> Result<i64> {
    if let Some(id) = db.character_by_starter(REN_KEY).await? {
        return Ok(id);
    }
    let avatar = format!("avatars/starter-{REN_KEY}.png");
    write_atomic(&paths.stored(&avatar)?, REN_AVATAR)?;
    db.insert_character(&NewCharacter {
        starter_key: Some(REN_KEY),
        name: "Ren Kagami",
        avatar_path: Some(&avatar),
        tagline: "A private detective who owes money to the wrong people.",
        personality: "Tired, sharp, sarcastic, has a quiet sense of honor. \
            He talks in short, dry sentences and hides his kindness behind jokes.",
        scenario: "{{user}} comes to his office at night with a case.",
        greeting: "Door was unlocked, so either you are brave or you are desperate. Sit. Talk.",
        example_dialogue: REN_EXAMPLES,
        voice_anchor: "Short, dry sentences. Sarcastic, tired, honest. Calls people \"kid\". \
            Never uses exclamation marks.",
        appearance_tags: "messy black hair, stubble, loose tie, long grey trench coat, \
            cigarette smoke, rain, noir",
        default_tone: "dark",
        default_reply_style: "conversational",
        age: 34,
    })
    .await
}

/// Writes to a temporary file, flushes it, then renames it over the target.
pub fn write_atomic(target: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = target.with_extension("tmp");
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, target)?;
    Ok(())
}
