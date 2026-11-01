//! Character card import: Character Card V1, V2 and V3, as plain JSON or as a
//! PNG with the card in a `tEXt` chunk. A card is untrusted input. This module
//! does no IO, so the import command, the tests and the fuzz target all run
//! the same code.
//!
//! The result is a draft for the creator, not a saved character. The user
//! reviews it, sets the age and saves, and the save checks every field again.

use crate::chat::is_bidi_control;
use crate::db::{CharacterFields, WorldEntry};
use crate::error::{Error, Result};
use crate::library::{
    MAX_AGE, MAX_ANCHOR, MAX_EXAMPLES, MAX_GREETING, MAX_NAME, MAX_PERSONALITY, MAX_SCENARIO,
    MAX_TAGLINE, MAX_TAGS,
};
use crate::presets::{STYLES, TONES};
use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Largest card file, in bytes.
pub const MAX_CARD_BYTES: usize = 20 * 1024 * 1024;
/// Largest card JSON after base64 decoding, in bytes.
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
/// Largest stored extensions block, in bytes.
pub const MAX_EXTENSIONS_BYTES: usize = 256 * 1024;
pub const MAX_WORLD_ENTRIES: usize = 200;
pub const MAX_WORLD_KEYWORDS: usize = 400;
pub const MAX_WORLD_CONTENT: usize = 2000;
/// The extensions key that keeps known card fields that Unstrung does not use.
/// They are never sent to the model. A later export can put them back.
pub const IMPORT_KEY: &str = "unstrung_import";
/// The extensions key for Unstrung's own fields.
pub const OWN_KEY: &str = "unstrung";

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The parts of a card that the creator does not show, kept for the save.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportExtras {
    pub extensions_json: String,
    pub world: Vec<WorldEntry>,
}

/// Things the user must know about an import before saving.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportNote {
    /// A field was longer than the limit, so its end was cut.
    Cut { field: &'static str },
    /// The card is marked 18+.
    Adult,
    /// The card has no age, so the user must enter one.
    NoAge,
    /// The card had its own system prompt or instructions. They are not used.
    DroppedPrompts,
    /// The card had other greetings. Only the first is used.
    MoreGreetings { count: usize },
    /// World info entries are kept for a later version.
    World { count: usize, dropped: usize },
    /// The PNG picture could not be used.
    NoPicture,
}

#[derive(Debug, Clone)]
pub struct ParsedCard {
    pub fields: CharacterFields,
    pub extras: ImportExtras,
    pub notes: Vec<ImportNote>,
    /// True when the card came in a PNG, which is also its picture.
    pub is_png: bool,
}

fn invalid(why: &str) -> Error {
    Error::Rejected {
        message:
            "this is not a character card that Unstrung can read. Pick a PNG or JSON card file"
                .into(),
        why: why.into(),
    }
}

/// Reads a card file: a PNG with a card chunk, or JSON.
pub fn parse_card(bytes: &[u8]) -> Result<ParsedCard> {
    if bytes.len() > MAX_CARD_BYTES {
        return Err(Error::Invalid("the card file is larger than 20 MB".into()));
    }
    let is_png = bytes.starts_with(PNG_SIGNATURE);
    let json = if is_png {
        decode_base64(png_card_text(bytes)?)?
    } else {
        if bytes.len() > MAX_JSON_BYTES {
            return Err(invalid("the JSON is too large"));
        }
        bytes.to_vec()
    };
    let value: Value =
        serde_json::from_slice(&json).map_err(|_| invalid("the card data is not valid JSON"))?;
    let mut card = from_value(&value)?;
    card.is_png = is_png;
    Ok(card)
}

/// Finds the card text in the PNG chunks. A `ccv3` chunk wins over `chara`.
/// Some writers put the text after the image data, so the walk goes to IEND.
fn png_card_text(bytes: &[u8]) -> Result<&[u8]> {
    let mut at = PNG_SIGNATURE.len();
    let mut chara: Option<&[u8]> = None;
    while let Some(head) = bytes.get(at..at.saturating_add(8)) {
        let mut len = [0u8; 4];
        len.copy_from_slice(&head[..4]);
        let len = usize::try_from(u32::from_be_bytes(len)).map_err(|_| invalid("bad PNG"))?;
        let kind = &head[4..8];
        let start = at + 8;
        let data = start
            .checked_add(len)
            .and_then(|end| bytes.get(start..end))
            .ok_or_else(|| invalid("the PNG is cut short"))?;
        if kind == b"tEXt"
            && let Some(nul) = data.iter().position(|b| *b == 0)
        {
            let (key, text) = (&data[..nul], &data[nul + 1..]);
            if key.eq_ignore_ascii_case(b"ccv3") {
                return Ok(text);
            }
            if key.eq_ignore_ascii_case(b"chara") && chara.is_none() {
                chara = Some(text);
            }
        }
        if kind == b"IEND" {
            break;
        }
        // Skip the data and the 4-byte CRC.
        at = start + len + 4;
    }
    chara.ok_or_else(|| invalid("the PNG has no card in it"))
}

fn decode_base64(text: &[u8]) -> Result<Vec<u8>> {
    let clean: Vec<u8> = text
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if clean.len() / 4 * 3 > MAX_JSON_BYTES {
        return Err(invalid("the card data is too large"));
    }
    let engine = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    engine
        .decode(&clean)
        .map_err(|_| invalid("the card data in the PNG is damaged"))
}

fn text_of<'a>(obj: &'a Map<String, Value>, key: &str) -> &'a str {
    obj.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Cleans card text: line endings, control and direction characters, and the
/// macro spellings that other apps use.
fn clean(text: &str) -> String {
    let text: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| (!c.is_control() || matches!(c, '\n' | '\t')) && !is_bidi_control(*c))
        .collect();
    normalize_macros(text.trim())
}

/// `{{Char}}`, `{{CHAR}}`, `<BOT>` and `<USER>` become `{{char}}` and `{{user}}`.
pub fn normalize_macros(text: &str) -> String {
    const MACROS: [(&str, &str); 5] = [
        ("{{char}}", "{{char}}"),
        ("{{user}}", "{{user}}"),
        ("<bot>", "{{char}}"),
        ("<char>", "{{char}}"),
        ("<user>", "{{user}}"),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '<']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let found = MACROS.iter().find(|(pattern, _)| {
            rest.get(..pattern.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(pattern))
        });
        if let Some((pattern, macro_text)) = found {
            out.push_str(macro_text);
            rest = &rest[pattern.len()..];
        } else {
            // `{` and `<` are one byte long.
            out.push_str(&rest[..1]);
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Keeps the first `max` characters. Counts characters, never bytes, so text
/// with emoji or CJK cannot be cut inside a character.
fn cut(text: String, max: usize, field: &'static str, notes: &mut Vec<ImportNote>) -> String {
    if text.chars().count() <= max {
        return text;
    }
    notes.push(ImportNote::Cut { field });
    text.chars()
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Removes the `<START>` lines that separate example chats in other apps.
fn clean_examples(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| {
            if l.trim().eq_ignore_ascii_case("<start>") {
                ""
            } else {
                l
            }
        })
        .collect();
    let mut out = String::new();
    let mut blank = true;
    for line in lines {
        if line.trim().is_empty() {
            if !blank {
                out.push('\n');
            }
            blank = true;
        } else {
            out.push_str(line);
            out.push('\n');
            blank = false;
        }
    }
    out.trim().to_owned()
}

fn from_value(value: &Value) -> Result<ParsedCard> {
    let root = value.as_object().ok_or_else(|| invalid("no card object"))?;
    // V2 and V3 keep the card in `data`. V1 has the fields at the top.
    let data = match root.get("data").and_then(Value::as_object) {
        Some(data) if data.get("name").is_some_and(Value::is_string) => data,
        _ => root,
    };
    let name = clean(text_of(data, "name"));
    if name.is_empty() {
        return Err(invalid("the card has no name"));
    }
    let mut notes = Vec::new();

    let description = clean(text_of(data, "description"));
    let personality = clean(text_of(data, "personality"));
    let body = match (description.is_empty(), personality.is_empty()) {
        (false, false) => format!("{description}\n\n{personality}"),
        (false, true) => description,
        _ => personality,
    };

    let empty = Map::new();
    let extensions = data
        .get("extensions")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let own = extensions
        .get(OWN_KEY)
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let own_text = |key: &str| clean(text_of(own, key));
    let preset = |key: &str, list: &[&str], default: &str| {
        let value = text_of(own, key);
        if list.contains(&value) && value != "custom" {
            value.to_owned()
        } else {
            default.to_owned()
        }
    };

    let tags: Vec<&str> = data
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| tags.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let tagged_adult = tags.iter().any(|t| {
        let t = t.trim();
        ["nsfw", "18+", "adult"]
            .iter()
            .any(|mark| t.eq_ignore_ascii_case(mark))
    });
    let is_adult = tagged_adult || own.get("is_adult").and_then(Value::as_bool) == Some(true);
    let age = own
        .get("age")
        .and_then(Value::as_i64)
        .filter(|a| (1..=MAX_AGE).contains(a))
        .unwrap_or(0);

    let fields = CharacterFields {
        name: cut(name.replace('\n', " "), MAX_NAME, "name", &mut notes),
        tagline: cut(
            own_text("tagline").replace('\n', " "),
            MAX_TAGLINE,
            "tagline",
            &mut notes,
        ),
        personality: cut(body, MAX_PERSONALITY, "personality", &mut notes),
        scenario: cut(
            clean(text_of(data, "scenario")),
            MAX_SCENARIO,
            "scenario",
            &mut notes,
        ),
        greeting: cut(
            clean(text_of(data, "first_mes")),
            MAX_GREETING,
            "greeting",
            &mut notes,
        ),
        example_dialogue: cut(
            clean_examples(&clean(text_of(data, "mes_example"))),
            MAX_EXAMPLES,
            "example_dialogue",
            &mut notes,
        ),
        voice_anchor: cut(
            own_text("voice_anchor").replace('\n', " "),
            MAX_ANCHOR,
            "voice_anchor",
            &mut notes,
        ),
        appearance_tags: cut(
            own_text("appearance_tags").replace('\n', " "),
            MAX_TAGS,
            "appearance_tags",
            &mut notes,
        ),
        default_tone: preset("default_tone", TONES, "casual"),
        default_reply_style: preset("default_reply_style", STYLES, "conversational"),
        default_reply_custom: String::new(),
        art_style: cut(
            own_text("art_style").replace('\n', " "),
            crate::library::MAX_ART_STYLE,
            "art_style",
            &mut notes,
        ),
        age,
        is_adult,
    };
    if is_adult {
        notes.push(ImportNote::Adult);
    }
    if age == 0 {
        notes.push(ImportNote::NoAge);
    }
    if !text_of(data, "system_prompt").trim().is_empty()
        || !text_of(data, "post_history_instructions").trim().is_empty()
    {
        notes.push(ImportNote::DroppedPrompts);
    }
    let greetings = data
        .get("alternate_greetings")
        .and_then(Value::as_array)
        .map_or(0, |g| g.iter().filter(|v| v.is_string()).count());
    if greetings > 0 {
        notes.push(ImportNote::MoreGreetings { count: greetings });
    }

    let (world, dropped) = world_entries(data);
    if !world.is_empty() || dropped > 0 {
        notes.push(ImportNote::World {
            count: world.len(),
            dropped,
        });
    }

    let extensions_json = extensions_with_dropped(extensions, data, &mut notes);
    Ok(ParsedCard {
        fields,
        extras: ImportExtras {
            extensions_json,
            world,
        },
        notes,
        is_png: false,
    })
}

/// World info from `character_book`. Returns the kept entries and the number
/// dropped because they were empty or over the limit.
fn world_entries(data: &Map<String, Value>) -> (Vec<WorldEntry>, usize) {
    let Some(entries) = data
        .get("character_book")
        .and_then(|b| b.get("entries"))
        .and_then(Value::as_array)
    else {
        return (Vec::new(), 0);
    };
    let mut kept = Vec::new();
    let mut dropped = 0;
    let mut ignore = Vec::new();
    for entry in entries {
        let Some(entry) = entry.as_object() else {
            dropped += 1;
            continue;
        };
        let content = clean(text_of(entry, "content"));
        if content.is_empty() || kept.len() >= MAX_WORLD_ENTRIES {
            dropped += 1;
            continue;
        }
        let keywords: Vec<String> = entry
            .get("keys")
            .and_then(Value::as_array)
            .map(|keys| {
                keys.iter()
                    .filter_map(Value::as_str)
                    .map(|k| clean(k).replace('\n', " "))
                    .filter(|k| !k.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        kept.push(WorldEntry {
            keywords: cut(
                keywords.join(", "),
                MAX_WORLD_KEYWORDS,
                "world",
                &mut ignore,
            ),
            content: cut(content, MAX_WORLD_CONTENT, "world", &mut ignore),
            is_enabled: entry.get("enabled").and_then(Value::as_bool) != Some(false),
        });
    }
    (kept, dropped)
}

/// The card's extensions block, plus the known fields that Unstrung does not
/// use, under `IMPORT_KEY`. Falls back to only those fields when the block is
/// too large.
fn extensions_with_dropped(
    extensions: &Map<String, Value>,
    data: &Map<String, Value>,
    notes: &mut Vec<ImportNote>,
) -> String {
    let mut dropped = Map::new();
    for key in [
        "system_prompt",
        "post_history_instructions",
        "alternate_greetings",
        "creator_notes",
        "creator",
        "character_version",
        "tags",
        "nickname",
    ] {
        if let Some(value) = data.get(key)
            && !value.is_null()
        {
            dropped.insert(key.to_owned(), value.clone());
        }
    }
    let mut all = extensions.clone();
    all.remove(OWN_KEY);
    all.insert(IMPORT_KEY.to_owned(), Value::Object(dropped.clone()));
    let json = Value::Object(all).to_string();
    if json.len() <= MAX_EXTENSIONS_BYTES {
        return json;
    }
    notes.push(ImportNote::Cut {
        field: "extensions",
    });
    let mut only = Map::new();
    only.insert(IMPORT_KEY.to_owned(), Value::Object(dropped));
    let json = Value::Object(only).to_string();
    if json.len() <= MAX_EXTENSIONS_BYTES {
        json
    } else {
        "{}".to_owned()
    }
}

#[cfg(test)]
mod tests;
