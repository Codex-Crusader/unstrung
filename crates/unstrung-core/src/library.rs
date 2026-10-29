//! Saving and deleting characters and personas. Every field from the UI is
//! checked here: required fields, lengths, known presets and ages.

use crate::avatar::{self, AvatarChange};
use crate::chat::{clean_line, is_bidi_control};
use crate::db::{Character, CharacterFields, Db, Persona, PersonaFields};
use crate::error::{Error, Result};
use crate::paths::AppPaths;
use crate::presets::{STYLES, TONES};

pub const MAX_NAME: usize = 80;
pub const MAX_TAGLINE: usize = 160;
pub const MAX_PERSONALITY: usize = 4000;
pub const MAX_SCENARIO: usize = 3000;
pub const MAX_GREETING: usize = 3000;
pub const MAX_EXAMPLES: usize = 6000;
pub const MAX_ANCHOR: usize = 400;
pub const MAX_TAGS: usize = 400;
pub const MAX_PERSONA_DESCRIPTION: usize = 600;
pub const MAX_AGE: i64 = 10_000;
pub const ADULT_AGE: i64 = 18;

/// Multi-line text: control and direction characters removed, trimmed, and at most `max` characters.
fn text(value: &str, max: usize, label: &str) -> Result<String> {
    let cleaned: String = value
        .trim()
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| (!c.is_control() || matches!(c, '\n' | '\t')) && !is_bidi_control(*c))
        .collect();
    if cleaned.chars().count() > max {
        return Err(Error::Invalid(format!(
            "{label} is longer than {max} characters"
        )));
    }
    Ok(cleaned)
}

fn line(value: &str, max: usize, label: &str) -> Result<String> {
    if value.trim().chars().count() > max {
        return Err(Error::Invalid(format!(
            "{label} is longer than {max} characters"
        )));
    }
    Ok(clean_line(value, max))
}

fn required(value: String, label: &str) -> Result<String> {
    if value.is_empty() {
        return Err(Error::Invalid(format!("{label} is required")));
    }
    Ok(value)
}

pub fn check_character(f: &CharacterFields) -> Result<CharacterFields> {
    let out = CharacterFields {
        name: required(line(&f.name, MAX_NAME, "the name")?, "the name")?,
        tagline: line(&f.tagline, MAX_TAGLINE, "the tagline")?,
        personality: required(
            text(&f.personality, MAX_PERSONALITY, "the personality")?,
            "the personality",
        )?,
        scenario: text(&f.scenario, MAX_SCENARIO, "the scenario")?,
        greeting: required(
            text(&f.greeting, MAX_GREETING, "the greeting")?,
            "the greeting",
        )?,
        example_dialogue: text(&f.example_dialogue, MAX_EXAMPLES, "the example dialogue")?,
        voice_anchor: line(&f.voice_anchor, MAX_ANCHOR, "the voice")?,
        appearance_tags: line(&f.appearance_tags, MAX_TAGS, "the appearance tags")?,
        default_tone: f.default_tone.clone(),
        default_reply_style: f.default_reply_style.clone(),
        age: f.age,
        is_adult: f.is_adult,
    };
    if !TONES.contains(&out.default_tone.as_str()) || out.default_tone == "custom" {
        return Err(Error::Invalid(format!("unknown tone {}", out.default_tone)));
    }
    if !STYLES.contains(&out.default_reply_style.as_str()) || out.default_reply_style == "custom" {
        return Err(Error::Invalid(format!(
            "unknown reply style {}",
            out.default_reply_style
        )));
    }
    if !(1..=MAX_AGE).contains(&out.age) {
        return Err(Error::Invalid(format!(
            "the age must be from 1 to {MAX_AGE}"
        )));
    }
    if out.is_adult && out.age < ADULT_AGE {
        return Err(Error::Invalid(
            "an 18+ character must be 18 or older".into(),
        ));
    }
    Ok(out)
}

pub fn check_persona(f: &PersonaFields) -> Result<PersonaFields> {
    let out = PersonaFields {
        name: required(line(&f.name, MAX_NAME, "the name")?, "the name")?,
        description: text(&f.description, MAX_PERSONA_DESCRIPTION, "the description")?,
        appearance_tags: line(&f.appearance_tags, MAX_TAGS, "the appearance tags")?,
        age: f.age,
    };
    if let Some(age) = out.age
        && !(1..=MAX_AGE).contains(&age)
    {
        return Err(Error::Invalid(format!(
            "the age must be from 1 to {MAX_AGE}"
        )));
    }
    Ok(out)
}

/// Makes the new picture first, so a failed picture does not leave a half-saved record.
fn new_picture(paths: &AppPaths, change: &AvatarChange, kind: &str) -> Result<Option<String>> {
    match change {
        AvatarChange::Staged { token, crop } => {
            Ok(Some(avatar::finish(paths, token, *crop, kind)?))
        }
        AvatarChange::Keep | AvatarChange::Remove => Ok(None),
    }
}

/// Creates a character (`id` is `None`) or changes one.
pub async fn save_character(
    db: &Db,
    paths: &AppPaths,
    id: Option<i64>,
    fields: &CharacterFields,
    change: &AvatarChange,
) -> Result<Character> {
    let fields = check_character(fields)?;
    let old = match id {
        Some(id) => Some(db.character(id).await?),
        None => None,
    };
    let picture = new_picture(paths, change, "character")?;
    let saved = async {
        let id = match id {
            Some(id) => {
                db.update_character(id, &fields).await?;
                id
            }
            None => db.insert_character(&fields, None, "{}").await?,
        };
        if picture.is_some() || matches!(change, AvatarChange::Remove) {
            db.set_character_avatar(id, picture.as_deref()).await?;
        }
        db.character(id).await
    }
    .await;
    match saved {
        Ok(character) => {
            if let Some(old_path) = old.and_then(|c| c.avatar_path)
                && character.avatar_path.as_deref() != Some(old_path.as_str())
            {
                avatar::remove(paths, &old_path);
            }
            Ok(character)
        }
        Err(e) => {
            if let Some(path) = picture {
                avatar::remove(paths, &path);
            }
            Err(e)
        }
    }
}

/// Deletes a character with its chats, messages, world info and picture.
pub async fn delete_character(db: &Db, paths: &AppPaths, id: i64) -> Result<()> {
    let character = db.character(id).await?;
    db.delete_character(id).await?;
    if let Some(path) = character.avatar_path {
        avatar::remove(paths, &path);
    }
    Ok(())
}

pub async fn save_persona(
    db: &Db,
    paths: &AppPaths,
    id: Option<i64>,
    fields: &PersonaFields,
    change: &AvatarChange,
) -> Result<Persona> {
    let fields = check_persona(fields)?;
    let old = match id {
        Some(id) => Some(db.persona(id).await?),
        None => None,
    };
    let picture = new_picture(paths, change, "persona")?;
    let saved = async {
        let id = match id {
            Some(id) => {
                db.update_persona(id, &fields).await?;
                id
            }
            None => db.insert_persona(&fields, None).await?,
        };
        if picture.is_some() || matches!(change, AvatarChange::Remove) {
            db.set_persona_avatar(id, picture.as_deref()).await?;
        }
        db.persona(id).await
    }
    .await;
    match saved {
        Ok(persona) => {
            if let Some(old_path) = old.and_then(|p| p.avatar_path)
                && persona.avatar_path.as_deref() != Some(old_path.as_str())
            {
                avatar::remove(paths, &old_path);
            }
            Ok(persona)
        }
        Err(e) => {
            if let Some(path) = picture {
                avatar::remove(paths, &path);
            }
            Err(e)
        }
    }
}

/// Deletes a persona. Chats and messages keep their text and lose the link.
pub async fn delete_persona(db: &Db, paths: &AppPaths, id: i64) -> Result<()> {
    let persona = db.persona(id).await?;
    db.delete_persona(id).await?;
    if let Some(path) = persona.avatar_path {
        avatar::remove(paths, &path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::Crop;
    use crate::db::NewMessage;

    fn fields() -> CharacterFields {
        CharacterFields {
            name: " Iris \n".into(),
            personality: "Calm.\r\nTalks slowly.".into(),
            greeting: "Hello.".into(),
            default_tone: "casual".into(),
            default_reply_style: "conversational".into(),
            age: 30,
            ..Default::default()
        }
    }

    #[test]
    fn checks() {
        let ok = check_character(&fields()).unwrap();
        assert_eq!(ok.name, "Iris");
        assert_eq!(ok.personality, "Calm.\nTalks slowly.");
        let bad = |change: fn(&mut CharacterFields)| {
            let mut f = fields();
            change(&mut f);
            check_character(&f).is_err()
        };
        assert!(bad(|f| f.name = "  ".into()));
        assert!(bad(|f| f.personality = String::new()));
        assert!(bad(|f| f.greeting = String::new()));
        assert!(bad(|f| f.age = 0));
        assert!(bad(|f| f.default_tone = "custom".into()));
        assert!(bad(|f| f.default_reply_style = "loud".into()));
        assert!(bad(|f| f.personality = "a".repeat(MAX_PERSONALITY + 1)));
        assert!(bad(|f| {
            f.is_adult = true;
            f.age = 17;
        }));
        assert!(
            check_persona(&PersonaFields {
                name: "Kai".into(),
                age: Some(0),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[tokio::test]
    async fn save_with_picture_and_delete_everything() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();

        let picture = image::DynamicImage::new_rgba8(200, 100);
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let staged = avatar::stage(&paths, &png).unwrap();
        let change = AvatarChange::Staged {
            token: staged.token,
            crop: Crop {
                x: 0,
                y: 0,
                size: 100,
            },
        };
        let saved = save_character(&db, &paths, None, &fields(), &change)
            .await
            .unwrap();
        let first = saved.avatar_path.clone().unwrap();
        assert!(paths.stored(&first).unwrap().is_file());

        // A second save with a new picture removes the old file.
        let staged = avatar::stage(&paths, &png).unwrap();
        let change = AvatarChange::Staged {
            token: staged.token,
            crop: Crop {
                x: 50,
                y: 0,
                size: 100,
            },
        };
        let saved = save_character(&db, &paths, Some(saved.id), &fields(), &change)
            .await
            .unwrap();
        let second = saved.avatar_path.clone().unwrap();
        assert!(!paths.stored(&first).unwrap().exists());

        let chat = db.create_chat(&saved, None, "", true).await.unwrap();
        let head = db.chat(chat).await.unwrap().head_message_id;
        db.append_message(&NewMessage {
            chat_id: chat,
            parent_id: head,
            role: "user",
            content: "hi",
            ..Default::default()
        })
        .await
        .unwrap();
        delete_character(&db, &paths, saved.id).await.unwrap();
        assert!(db.chat(chat).await.is_err(), "the chats go too");
        assert!(
            db.tree_links(chat).await.unwrap().is_empty(),
            "and the messages"
        );
        assert!(!paths.stored(&second).unwrap().exists(), "and the picture");
    }
}
