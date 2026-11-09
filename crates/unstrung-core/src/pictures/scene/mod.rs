//! The scene step: the chat model reads the last messages and describes the
//! moment as a short JSON object. The core then joins it with the fixed
//! appearance tags of the people in it. The chat model only says what
//! happens, never what a person looks like.

use super::safety::Rules;
use serde::{Deserialize, Serialize};

mod prompts;
#[cfg(test)]
mod tests;

pub use prompts::*;

/// The messages that the scene step reads.
pub const RECENT_MESSAGES: usize = 8;
/// Each field is cut to this many characters.
const MAX_FIELD: usize = 200;

/// Who is in the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    Character,
    Persona,
    Both,
    /// A place or object with no one in it.
    Nobody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub setting: String,
    pub action: String,
    pub mood: String,
    pub camera: String,
    pub who: Who,
    /// The model's own flag for a sexual or nude scene. The content rules
    /// treat it as one more signal and never as permission.
    #[serde(default)]
    pub sexual: bool,
    /// Clothes that the story changed, for each person. Empty: the usual outfit.
    #[serde(default)]
    pub character_outfit: String,
    #[serde(default)]
    pub persona_outfit: String,
}

/// The JSON schema for the `format` option of the chat request.
pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "setting": { "type": "string" },
            "action": { "type": "string" },
            "mood": { "type": "string" },
            "camera": { "type": "string" },
            "who": { "type": "string", "enum": ["character", "persona", "both", "nobody"] },
            "sexual": { "type": "boolean" },
            "character_outfit": { "type": "string" },
            "persona_outfit": { "type": "string" }
        },
        "required": ["setting", "action", "mood", "camera", "who", "sexual", "character_outfit", "persona_outfit"]
    })
}

pub const SYSTEM: &str = "You describe one moment of a roleplay chat for a painter. \
Answer with one JSON object only.";

/// The question for the chat model. `lines` are the recent messages as
/// (speaker name, text), oldest first. `wish` is the user's own `/imagine` text.
pub fn question(character: &str, user: &str, lines: &[(String, String)], wish: &str) -> String {
    let mut q = String::from("The chat so far:\n\n");
    for (name, text) in lines {
        q.push_str(&format!("{name}: {text}\n\n"));
    }
    q.push_str(&format!(
        "Describe the latest moment of this chat as a picture. {character} is the \
character. {user} is the user.\n\
- setting: the place, time of day and light, in a few words.\n\
- who: who can be seen in the picture. The picture shows the moment as the user sees it, so the user is usually not in it.\n\
  \"character\": the character is alone, or the user is somewhere else (another room, a radio, a screen), or the character acts or talks while the user only watches or listens.\n\
  \"both\": the two are in the same place and the moment needs both of them: they touch, play a game, eat or drink together, dance, fight, or one hands the other something.\n\
  \"persona\": only the user can be seen.\n\
  \"nobody\": a place or a thing with no people.\n\
- action: what the people who can be seen do, in one short sentence. Use \"the character\" and \"the user\", never names. Never mention a person who cannot be seen.\n\
- mood: one to three words.\n\
- camera: the shot, for example \"close-up\", \"medium shot\" or \"wide shot\".\n\
- sexual: true if the moment is sexual or shows nudity, else false.\n\
- character_outfit: what the character wears now, only if the chat says that \
the character changed clothes, for example \"a red evening dress\". Else \
an empty string.\n\
- persona_outfit: the same for the user.\n\
Do not describe faces, hair or bodies: the painter already knows them. \
Write in English.\n"
    ));
    if !wish.trim().is_empty() {
        q.push_str(&format!("The user wants this picture: {}\n", wish.trim()));
    }
    q
}

/// Reads the model's answer. `None` when it is not a usable scene.
pub fn parse(answer: &str) -> Option<Scene> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let mut scene: Scene = serde_json::from_str(answer.get(start..=end)?).ok()?;
    for field in [
        &mut scene.setting,
        &mut scene.action,
        &mut scene.mood,
        &mut scene.camera,
        &mut scene.character_outfit,
        &mut scene.persona_outfit,
    ] {
        *field = clean(field);
    }
    (!scene.action.is_empty() || !scene.setting.is_empty()).then_some(scene)
}

/// Clears each story outfit whose clothing word is not in `said`, the recent
/// chat text. "utility jumpsuit" in a chat about an engine is made up; "a long
/// red evening dress" after "she comes back in a red evening dress" is not.
pub fn keep_said_outfits(scene: &mut Scene, said: &str) {
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .map(|w| w.strip_suffix('s').unwrap_or(w).to_owned())
            .collect()
    };
    let said = words(said);
    for outfit in [&mut scene.character_outfit, &mut scene.persona_outfit] {
        // The last word of each piece names the clothes: "a red evening
        // dress", "a grey suit with a tie" -> dress, suit, tie.
        let lower = outfit.to_lowercase();
        let named = lower
            .split([',', ';'])
            .flat_map(|piece| piece.split(" and "))
            .flat_map(|piece| piece.split(" with "))
            .filter_map(|piece| words(piece).pop())
            .any(|head| said.contains(&head));
        if !named {
            outfit.clear();
        }
    }
}

/// The plain scene when the model gives no usable answer: the last message
/// as the action, with both people in it.
pub fn fallback(last_message: &str, wish: &str) -> Scene {
    let action = if wish.trim().is_empty() {
        last_message
    } else {
        wish
    };
    Scene {
        setting: String::new(),
        action: clean(action),
        mood: String::new(),
        camera: "medium shot".into(),
        who: Who::Both,
        sexual: false,
        character_outfit: String::new(),
        persona_outfit: String::new(),
    }
}

/// One line, no markup, at most `MAX_FIELD` characters, cut at a word.
fn clean(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '<' | '>' | '[' | ']' | '{' | '}'))
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_FIELD {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_FIELD).collect();
    match cut.rfind(' ') {
        Some(i) => cut[..i].to_owned(),
        None => cut,
    }
}
