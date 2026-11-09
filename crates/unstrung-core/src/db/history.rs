//! Reads across all chats: the numbers per character and the search in
//! message text.

use super::Db;
use crate::error::Result;
use serde::Serialize;

/// The numbers of one character, for Settings > Storage.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CharacterStats {
    pub character_id: i64,
    pub name: String,
    pub chats: i64,
    /// Messages of the user and the character, without deleted ones.
    pub messages: i64,
    pub pictures: i64,
}

impl Db {
    /// Every character with its chats, messages and pictures, most messages
    /// first. Deleted chats do not count. Adult characters only when `adult`.
    pub async fn stats(&self, adult: bool) -> Result<Vec<CharacterStats>> {
        Ok(sqlx::query_as!(
            CharacterStats,
            r#"SELECT ch.id AS "character_id!", ch.name AS "name!",
                    (SELECT count(*) FROM chats c
                     WHERE c.character_id = ch.id AND c.deleted_at IS NULL) AS "chats!: i64",
                    (SELECT count(*) FROM messages m JOIN chats c ON c.id = m.chat_id
                     WHERE c.character_id = ch.id AND c.deleted_at IS NULL
                       AND m.deleted_at IS NULL AND m.role IN ('user', 'character'))
                        AS "messages!: i64",
                    (SELECT count(*) FROM images i JOIN chats c ON c.id = i.chat_id
                     WHERE c.character_id = ch.id AND c.deleted_at IS NULL) AS "pictures!: i64"
             FROM characters ch
             WHERE ch.is_adult = 0 OR ?
             ORDER BY 4 DESC, ch.name"#,
            adult
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

/// The shortest search text: shorter ones match nearly every message.
pub const MIN_SEARCH_CHARS: usize = 3;
/// The most hits one search returns, newest first.
pub const SEARCH_LIMIT: i64 = 50;
/// Bytes of text around a hit in its snippet.
const BEFORE: usize = 60;
const AFTER: usize = 120;

/// A message that holds the search text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchHit {
    pub chat_id: i64,
    pub message_id: i64,
    pub character_id: i64,
    pub character_name: String,
    pub avatar_path: Option<String>,
    pub created_at: i64,
    /// The text around the hit: `before`, the hit itself, `after`. A cut
    /// end has "…".
    pub before: String,
    pub hit: String,
    pub after: String,
}

impl Db {
    /// Messages of live chats that hold `text`, newest first. Like SQLite
    /// LIKE, the match ignores case only for ASCII letters. Adult characters
    /// only when `adult`. Text shorter than `MIN_SEARCH_CHARS` finds nothing.
    pub async fn search(&self, text: &str, adult: bool) -> Result<Vec<SearchHit>> {
        let text = text.trim();
        if text.chars().count() < MIN_SEARCH_CHARS {
            return Ok(Vec::new());
        }
        let pattern = format!("%{}%", like_escape(text));
        // shortcut: LIKE reads every message, which is fast enough for tens
        // of thousands of messages; an FTS5 table is the upgrade if not.
        let rows = sqlx::query!(
            r#"SELECT m.id AS "message_id!", m.chat_id AS "chat_id!", m.content AS "content!",
                    m.created_at AS "created_at!", ch.id AS "character_id!",
                    ch.name AS "character_name!", ch.avatar_path
             FROM messages m
             JOIN chats c ON c.id = m.chat_id
             JOIN characters ch ON ch.id = c.character_id
             WHERE c.deleted_at IS NULL AND m.deleted_at IS NULL
               AND (ch.is_adult = 0 OR ?)
               AND m.role IN ('user', 'character', 'ooc')
               AND m.content LIKE ? ESCAPE '\'
             ORDER BY m.created_at DESC, m.id DESC
             LIMIT ?"#,
            adult,
            pattern,
            SEARCH_LIMIT
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let (before, hit, after) = snippet(&r.content, text);
                SearchHit {
                    chat_id: r.chat_id,
                    message_id: r.message_id,
                    character_id: r.character_id,
                    character_name: r.character_name,
                    avatar_path: r.avatar_path,
                    created_at: r.created_at,
                    before,
                    hit,
                    after,
                }
            })
            .collect())
    }
}

/// `%`, `_` and `\` as plain characters in a LIKE pattern with ESCAPE '\'.
fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The text around the first match of `wanted`, as LIKE finds it: ASCII
/// letters in any case. ASCII lowercase keeps every byte offset, so the
/// offsets fit `content`.
fn snippet(content: &str, wanted: &str) -> (String, String, String) {
    let at = content
        .to_ascii_lowercase()
        .find(&wanted.to_ascii_lowercase());
    let Some(start) = at else {
        let end = floor(content, AFTER);
        let more = if end < content.len() { "…" } else { "" };
        return (
            String::new(),
            String::new(),
            format!("{}{more}", &content[..end]),
        );
    };
    let end = start + wanted.len();
    let from = floor(content, start.saturating_sub(BEFORE));
    let to = floor(content, end + AFTER);
    let lead = if from > 0 { "…" } else { "" };
    let tail = if to < content.len() { "…" } else { "" };
    (
        format!("{lead}{}", &content[from..start]),
        content[start..end].to_string(),
        format!("{}{tail}", &content[end..to]),
    )
}

/// The nearest char boundary at or below `at`, never past the end.
fn floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// One chat as Markdown, for "Save as text".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// A file name without folders or characters that Windows refuses.
    pub file_name: String,
    pub text: String,
}

impl Db {
    /// The visible branch of a chat as Markdown: one paragraph per message,
    /// with the name of who wrote it. System notes stay out.
    pub async fn transcript(&self, chat_id: i64) -> Result<Transcript> {
        let chat = self.chat(chat_id).await?;
        let character = self.character(chat.character_id).await?;
        let personas: std::collections::HashMap<i64, String> = self
            .personas()
            .await?
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect();
        let mut text = format!("# {}\n", character.name);
        for m in self.path_to_head(chat_id).await? {
            let name = match m.role.as_str() {
                "character" => character.name.as_str(),
                "user" | "ooc" => m
                    .persona_id
                    .and_then(|id| personas.get(&id))
                    .map_or(crate::prompt::NO_PERSONA_NAME, String::as_str),
                _ => continue,
            };
            let content = m.content.trim();
            if content.is_empty() {
                continue;
            }
            let note = if m.role == "ooc" {
                " (out of character)"
            } else {
                ""
            };
            text.push_str(&format!("\n**{name}{note}:** {content}\n"));
        }
        Ok(Transcript {
            file_name: format!("{}.md", file_stem(&character.name)),
            text,
        })
    }
}

/// A character name as a Windows file name: no folder marks, no reserved
/// characters, no dot or space at the end, and never empty.
fn file_stem(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']);
    if clean.is_empty() {
        "Chat".into()
    } else {
        format!("Chat with {clean}")
    }
}

#[cfg(test)]
mod tests {
    use super::{file_stem, like_escape, snippet};

    #[test]
    fn snippets_cut_at_char_boundaries_and_keep_the_case() {
        let (b, h, a) = snippet("Hello World", "world");
        assert_eq!(
            (b.as_str(), h.as_str(), a.as_str()),
            ("Hello ", "World", "")
        );
        // Multi-byte letters around the hit: the cut never splits one.
        let long = format!("{}KEY{}", "é".repeat(100), "ü".repeat(100));
        let (b, h, a) = snippet(&long, "key");
        assert_eq!(h, "KEY");
        assert!(b.starts_with('…') && a.ends_with('…'));
        assert!(b.len() <= 60 + '…'.len_utf8() && a.len() <= 120 + '…'.len_utf8());
        // Not found (LIKE and Rust disagree): the start of the text.
        let (b, h, a) = snippet("Ärger", "zzz");
        assert_eq!((b.as_str(), h.as_str(), a.as_str()), ("", "", "Ärger"));
    }

    #[test]
    fn like_wildcards_are_plain_text() {
        assert_eq!(like_escape(r"50%_a\b"), r"50\%\_a\\b");
    }

    #[test]
    fn file_stems_are_safe_on_windows() {
        assert_eq!(file_stem("Ren"), "Chat with Ren");
        assert_eq!(file_stem("A/B:C?"), "Chat with A B C");
        assert_eq!(file_stem(r"..\.."), "Chat");
        assert_eq!(file_stem("Dr. Who.  "), "Chat with Dr. Who");
        assert_eq!(file_stem(""), "Chat");
    }
}
