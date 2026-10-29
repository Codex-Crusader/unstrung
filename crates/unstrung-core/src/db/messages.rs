use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub role: String,
    pub persona_id: Option<i64>,
    pub content: String,
    pub flagged_out_of_character: bool,
    pub model: Option<String>,
    pub created_at: i64,
}

/// A message that is not saved yet.
#[derive(Debug, Default)]
pub struct NewMessage<'a> {
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub role: &'a str,
    pub persona_id: Option<i64>,
    pub content: &'a str,
    pub raw_content: Option<&'a str>,
    pub token_count: Option<i64>,
    pub model: Option<&'a str>,
    pub prompt_version: Option<i64>,
    pub sampler_json: Option<&'a str>,
}

impl Db {
    /// A message of this chat that is not deleted.
    pub async fn message(&self, chat_id: i64, id: i64) -> Result<Message> {
        sqlx::query_as!(
            Message,
            r#"SELECT id, chat_id, parent_id, role, persona_id, content,
                    flagged_out_of_character AS "flagged_out_of_character: bool", model, created_at
             FROM messages WHERE id = ? AND chat_id = ? AND deleted_at IS NULL"#,
            id,
            chat_id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("message"))
    }

    /// Saves a message under its parent and makes it the new head of the chat.
    pub async fn append_message(&self, m: &NewMessage<'_>) -> Result<Message> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO messages (chat_id, parent_id, role, persona_id, content, raw_content,
                    token_count, model, prompt_version, sampler_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            m.chat_id,
            m.parent_id,
            m.role,
            m.persona_id,
            m.content,
            m.raw_content,
            m.token_count,
            m.model,
            m.prompt_version,
            m.sampler_json,
            now
        )
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE chats SET head_message_id = ?, updated_at = ? WHERE id = ?",
            id,
            now,
            m.chat_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Message {
            id,
            chat_id: m.chat_id,
            parent_id: m.parent_id,
            role: m.role.to_owned(),
            persona_id: m.persona_id,
            content: m.content.to_owned(),
            flagged_out_of_character: false,
            model: m.model.map(str::to_owned),
            created_at: now,
        })
    }

    /// The messages from the first message to the head, oldest first.
    /// The model output before cleanup, for the quality suite. `None` for user messages.
    pub async fn raw_content(&self, id: i64) -> Result<Option<String>> {
        let raw = sqlx::query_scalar!("SELECT raw_content FROM messages WHERE id = ?", id)
            .fetch_one(&self.pool)
            .await?;
        Ok(raw)
    }

    pub async fn path_to_head(&self, chat_id: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query_as!(
            Message,
            r#"WITH RECURSIVE path(id, depth) AS (
                 SELECT head_message_id, 0 FROM chats WHERE id = ?1 AND head_message_id IS NOT NULL
                 UNION ALL
                 SELECT m.parent_id, path.depth + 1 FROM messages m JOIN path ON m.id = path.id
                 WHERE m.parent_id IS NOT NULL AND path.depth < 100000
             )
             SELECT m.id AS "id!", m.chat_id AS "chat_id!", m.parent_id, m.role AS "role!",
                    m.persona_id, m.content AS "content!",
                    m.flagged_out_of_character AS "flagged_out_of_character!: bool",
                    m.model, m.created_at AS "created_at!"
             FROM path JOIN messages m ON m.id = path.id AND m.chat_id = ?1
             WHERE m.deleted_at IS NULL
             ORDER BY path.depth DESC"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// `(id, parent_id)` of every message that is not deleted, oldest first.
    pub async fn tree_links(&self, chat_id: i64) -> Result<Vec<(i64, Option<i64>)>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", parent_id FROM messages
             WHERE chat_id = ? AND deleted_at IS NULL ORDER BY id"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.id, r.parent_id)).collect())
    }

    pub async fn update_message_content(
        &self,
        id: i64,
        content: &str,
        raw_content: Option<&str>,
        token_count: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE messages SET content = ?, raw_content = COALESCE(?, raw_content),
                    token_count = COALESCE(?, token_count)
             WHERE id = ?",
            content,
            raw_content,
            token_count,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn flag_out_of_character(&self, id: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE messages SET flagged_out_of_character = 1 WHERE id = ?",
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Marks a message and all messages under it as deleted, with the time `stamp`.
    pub async fn delete_subtree(&self, chat_id: i64, id: i64, stamp: i64) -> Result<()> {
        sqlx::query!(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM messages WHERE id = ?1 AND chat_id = ?2
                 UNION ALL
                 SELECT m.id FROM messages m JOIN sub ON m.parent_id = sub.id
             )
             UPDATE messages SET deleted_at = ?3
             WHERE id IN (SELECT id FROM sub) AND deleted_at IS NULL",
            id,
            chat_id,
            stamp
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Undoes `delete_subtree` with the same `stamp`.
    pub async fn restore_subtree(&self, chat_id: i64, id: i64, stamp: i64) -> Result<()> {
        sqlx::query!(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM messages WHERE id = ?1 AND chat_id = ?2
                 UNION ALL
                 SELECT m.id FROM messages m JOIN sub ON m.parent_id = sub.id
             )
             UPDATE messages SET deleted_at = NULL
             WHERE id IN (SELECT id FROM sub) AND deleted_at = ?3",
            id,
            chat_id,
            stamp
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Copies a path of messages into another chat and makes the last one the head.
    pub async fn copy_path(&self, chat_id: i64, path: &[Message]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let mut parent: Option<i64> = None;
        for m in path {
            let id = sqlx::query_scalar!(
                r#"INSERT INTO messages (chat_id, parent_id, role, persona_id, content, raw_content,
                        token_count, model, prompt_version, sampler_json, created_at)
                 SELECT ?, ?, role, persona_id, content, raw_content, token_count, model,
                        prompt_version, sampler_json, created_at
                 FROM messages WHERE id = ?
                 RETURNING id AS "id!""#,
                chat_id,
                parent,
                m.id
            )
            .fetch_one(&mut *tx)
            .await?;
            parent = Some(id);
        }
        sqlx::query!(
            "UPDATE chats SET head_message_id = ? WHERE id = ?",
            parent,
            chat_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
