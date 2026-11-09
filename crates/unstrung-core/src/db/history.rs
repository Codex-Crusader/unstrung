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
    use super::file_stem;

    #[test]
    fn file_stems_are_safe_on_windows() {
        assert_eq!(file_stem("Ren"), "Chat with Ren");
        assert_eq!(file_stem("A/B:C?"), "Chat with A B C");
        assert_eq!(file_stem(r"..\.."), "Chat");
        assert_eq!(file_stem("Dr. Who.  "), "Chat with Dr. Who");
        assert_eq!(file_stem(""), "Chat");
    }
}
