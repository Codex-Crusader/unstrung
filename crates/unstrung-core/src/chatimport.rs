//! Imports a chat from SillyTavern's `.jsonl` format: one JSON object per
//! line. The first line can be a header without `mes`. Each message line has
//! `mes` (the text, already the chosen swipe), `is_user` and `is_system`.
//! The file comes from outside, so every line is checked, and every message
//! goes through the same check as a message that the user types.

use crate::chat::clean_input;
use crate::db::{Character, Db};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Bigger files are refused before they are read.
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// The most messages one import takes.
pub const MAX_MESSAGES: usize = 20_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Imported {
    pub chat_id: i64,
    pub messages: usize,
    /// System notes and empty messages (for example a picture only).
    pub skipped: usize,
}

#[derive(Deserialize)]
struct Line {
    mes: Option<serde_json::Value>,
    #[serde(default)]
    is_user: bool,
    #[serde(default)]
    is_system: bool,
}

/// The messages of a file, as (role, text), oldest first, and the number of
/// lines skipped. An error names the line.
pub fn parse(bytes: &[u8]) -> Result<(Vec<(&'static str, String)>, usize)> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid("the file is not a SillyTavern chat (.jsonl)".into()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut messages = Vec::new();
    let mut skipped = 0;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| Error::Invalid(format!("line {number} of the file {why}"));
        let parsed: Line =
            serde_json::from_str(line).map_err(|_| bad("is not a SillyTavern chat message"))?;
        let mes = match parsed.mes {
            // The header line.
            None if number == 1 => continue,
            None => return Err(bad("has no message text")),
            Some(serde_json::Value::String(s)) => s,
            Some(_) => return Err(bad("is not a SillyTavern chat message")),
        };
        if parsed.is_system || mes.trim().is_empty() {
            skipped += 1;
            continue;
        }
        let content = match clean_input(&mes) {
            Ok(c) => c,
            // The hard limit: the whole import stops, with the same message.
            Err(e @ Error::Rejected { .. }) => return Err(e),
            Err(Error::Invalid(why)) => return Err(bad(&format!("is refused: {why}"))),
            Err(e) => return Err(e),
        };
        if messages.len() == MAX_MESSAGES {
            return Err(Error::Invalid(format!(
                "the file has more than {MAX_MESSAGES} messages"
            )));
        }
        let role = if parsed.is_user { "user" } else { "character" };
        messages.push((role, content));
    }
    if messages.is_empty() {
        return Err(Error::Invalid("the file has no chat messages".into()));
    }
    Ok((messages, skipped))
}

/// Imports the file as a new chat with `character`, with the user messages
/// under `persona_id`. All or nothing.
pub async fn import(
    db: &Db,
    character: &Character,
    persona_id: Option<i64>,
    bytes: &[u8],
) -> Result<Imported> {
    let (messages, skipped) = parse(bytes)?;
    let chat_id = db
        .insert_imported_chat(character, persona_id, &messages)
        .await?;
    log::info!(
        "chat imported: {} messages, {skipped} skipped",
        messages.len()
    );
    Ok(Imported {
        chat_id,
        messages: messages.len(),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(all: &[&str]) -> Vec<u8> {
        all.join("\n").into_bytes()
    }

    #[test]
    fn a_chat_reads_with_its_header_and_skips_system_notes() {
        let file = lines(&[
            r#"{"user_name":"You","character_name":"Ren","create_date":"x","chat_metadata":{}}"#,
            r#"{"name":"Ren","is_user":false,"mes":"Sit. Talk.","swipes":["Sit. Talk.","Hi"],"swipe_id":0}"#,
            "",
            r#"{"name":"You","is_user":true,"is_system":false,"mes":"  Hello\u0007 "}"#,
            r#"{"name":"System","is_user":false,"is_system":true,"mes":"A note"}"#,
            r#"{"name":"Ren","is_user":false,"mes":"","extra":{"image":"a.png"}}"#,
        ]);
        let (messages, skipped) = parse(&file).unwrap();
        assert_eq!(
            messages,
            [
                ("character", "Sit. Talk.".to_string()),
                ("user", "Hello".to_string())
            ]
        );
        assert_eq!(skipped, 2);
    }

    #[test]
    fn bad_files_name_the_line_and_never_panic() {
        let err = |file: Vec<u8>| parse(&file).unwrap_err().to_string();
        assert!(err(lines(&[r#"{"mes":"ok"}"#, "not json"])).contains("line 2"));
        assert!(
            err(lines(&[
                r#"{"mes":"ok"}"#,
                r#"{"mes":"x","is_user":"yes"}"#
            ]))
            .contains("line 2")
        );
        assert!(
            err(lines(&[r#"{"mes":"ok"}"#, r#"{"mes":{"message":"chub"}}"#])).contains("line 2")
        );
        assert!(err(lines(&[r#"{"mes":"ok"}"#, r#"{"name":"x"}"#])).contains("line 2"));
        assert!(err(lines(&[r#"{"chat_metadata":{}}"#])).contains("no chat messages"));
        assert!(err(vec![0xff, 0xfe, 0x00]).contains("not a SillyTavern chat"));
        let long = format!(
            r#"{{"mes":"{}"}}"#,
            "a".repeat(crate::chat::MAX_MESSAGE_CHARS + 1)
        );
        assert!(err(long.into_bytes()).contains("line 1"));
        assert!(err(b"[1,2]".to_vec()).contains("line 1"));
    }

    #[test]
    fn the_hard_limit_stops_the_whole_import() {
        let file = lines(&[
            r#"{"mes":"Fine."}"#,
            r#"{"mes":"*He undressed the schoolgirl.*","is_user":true}"#,
        ]);
        assert!(matches!(parse(&file), Err(Error::Rejected { .. })));
    }

    #[tokio::test]
    async fn an_import_is_one_branch_that_opens_like_any_chat() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db"), &crate::vault::DataKey::test())
            .await
            .unwrap();
        let fields = crate::db::CharacterFields {
            name: "Ren".into(),
            greeting: "Sit. Talk.".into(),
            default_tone: "dark".into(),
            default_reply_style: "conversational".into(),
            age: 34,
            ..Default::default()
        };
        let id = db.insert_character(&fields, None, "{}").await.unwrap();
        let ren = db.character(id).await.unwrap();
        let file = lines(&[
            r#"{"chat_metadata":{}}"#,
            r#"{"mes":"I need help.","is_user":true}"#,
            r#"{"mes":"With what?"}"#,
            r#"{"mes":"A watch.","is_user":true}"#,
        ]);
        let done = import(&db, &ren, None, &file).await.unwrap();
        assert_eq!((done.messages, done.skipped), (3, 0));
        let path = db.path_to_head(done.chat_id).await.unwrap();
        let roles: Vec<_> = path
            .iter()
            .map(|m| (m.role.as_str(), m.content.as_str()))
            .collect();
        assert_eq!(
            roles,
            [
                ("user", "I need help."),
                ("character", "With what?"),
                ("user", "A watch.")
            ]
        );
        assert_eq!(path[0].parent_id, None);
        // It lists with the other chats.
        let recent = db.recent_chats(10, false).await.unwrap();
        assert_eq!(recent.first().map(|r| r.chat_id), Some(done.chat_id));
        // A refused file leaves no chat behind.
        let before = db.chat_summaries(id).await.unwrap().len();
        assert!(
            import(&db, &ren, None, &lines(&[r#"{"mes":"ok"}"#, "{"]))
                .await
                .is_err()
        );
        assert_eq!(db.chat_summaries(id).await.unwrap().len(), before);
    }
}
