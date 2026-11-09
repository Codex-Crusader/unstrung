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
