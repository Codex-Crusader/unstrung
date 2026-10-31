use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::Serialize;

/// A memory of a chat: the user's goal (`goal`), the story so far (`summary`)
/// or one fact (`fact`).
#[derive(Debug, Clone, Serialize)]
pub struct Memory {
    pub id: i64,
    pub chat_id: i64,
    pub kind: String,
    pub content: String,
    /// The message it came from. A summary covers the path up to this message.
    pub source_message_id: i64,
    /// "auto" from the memory pass, "user" from the user.
    pub origin: String,
    pub is_pinned: bool,
    pub is_hidden: bool,
    pub created_at: i64,
}

impl Db {
    /// All memories of a chat, oldest first, hidden ones too.
    pub async fn memories(&self, chat_id: i64) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as!(
            Memory,
            r#"SELECT id AS "id!", chat_id, kind, content, source_message_id, origin,
                    is_pinned AS "is_pinned: bool", is_hidden AS "is_hidden: bool", created_at
             FROM memories WHERE chat_id = ? ORDER BY id"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn add_memory(
        &self,
        chat_id: i64,
        kind: &str,
        content: &str,
        source_message_id: i64,
        origin: &str,
    ) -> Result<Memory> {
        let now = now_ms();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO memories (chat_id, kind, content, source_message_id, origin, created_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            chat_id,
            kind,
            content,
            source_message_id,
            origin,
            now
        )
        .fetch_one(&self.pool)
        .await?;
        self.memory(chat_id, id).await
    }

    pub async fn memory(&self, chat_id: i64, id: i64) -> Result<Memory> {
        sqlx::query_as!(
            Memory,
            r#"SELECT id AS "id!", chat_id, kind, content, source_message_id, origin,
                    is_pinned AS "is_pinned: bool", is_hidden AS "is_hidden: bool", created_at
             FROM memories WHERE id = ? AND chat_id = ?"#,
            id,
            chat_id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("memory"))
    }

    /// Changes the text of a memory. An edit makes it the user's.
    pub async fn edit_memory(&self, chat_id: i64, id: i64, content: &str) -> Result<Memory> {
        sqlx::query!(
            "UPDATE memories SET content = ?, origin = 'user' WHERE id = ? AND chat_id = ?",
            content,
            id,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        self.memory(chat_id, id).await
    }

    pub async fn set_memory_flags(
        &self,
        chat_id: i64,
        id: i64,
        pinned: bool,
        hidden: bool,
    ) -> Result<Memory> {
        sqlx::query!(
            "UPDATE memories SET is_pinned = ?, is_hidden = ? WHERE id = ? AND chat_id = ?",
            pinned,
            hidden,
            id,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        self.memory(chat_id, id).await
    }
}
