use super::{Character, Db, now_ms};
use crate::error::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Chat {
    pub id: i64,
    pub character_id: i64,
    pub persona_id: Option<i64>,
    pub title: String,
    pub tone: String,
    pub tone_custom: String,
    pub reply_style: String,
    pub reply_custom: String,
    pub chat_model: Option<String>,
    pub head_message_id: Option<i64>,
    /// "Refresh context": the prompt starts near this message (`memory::window`).
    pub context_start_id: Option<i64>,
    /// The chat uses the character's kept memories.
    pub use_character_memory: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A chat for the "Recent chats" row on Home.
#[derive(Debug, Clone, Serialize)]
pub struct RecentChat {
    pub chat_id: i64,
    pub character_id: i64,
    pub character_name: String,
    pub avatar_path: Option<String>,
    /// The start of the last message on the current branch.
    pub preview: String,
    pub updated_at: i64,
}

/// A chat in "Recently deleted" on the Chats screen.
#[derive(Debug, Clone, Serialize)]
pub struct DeletedChat {
    pub chat_id: i64,
    pub character_id: i64,
    pub character_name: String,
    pub avatar_path: Option<String>,
    pub preview: String,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatSummary {
    pub id: i64,
    pub title: String,
    pub preview: String,
    pub message_count: i64,
    pub updated_at: i64,
}

impl Db {
    pub async fn chat(&self, id: i64) -> Result<Chat> {
        sqlx::query_as!(
            Chat,
            r#"SELECT id, character_id, persona_id, title, tone, tone_custom, reply_style,
                    reply_custom, chat_model, head_message_id, context_start_id,
                    use_character_memory AS "use_character_memory: bool", created_at, updated_at
             FROM chats WHERE id = ? AND deleted_at IS NULL"#,
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("chat"))
    }

    /// The most recently used chat with a character.
    pub async fn latest_chat(&self, character_id: i64) -> Result<Option<i64>> {
        let id = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM chats WHERE character_id = ? AND deleted_at IS NULL
             ORDER BY updated_at DESC, id DESC LIMIT 1"#,
            character_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// All chats with a character, newest first.
    pub async fn chat_summaries(&self, character_id: i64) -> Result<Vec<ChatSummary>> {
        let rows = sqlx::query_as!(
            ChatSummary,
            r#"SELECT c.id AS "id!", c.title AS "title!", c.updated_at AS "updated_at!",
                    COALESCE((SELECT substr(m.content, 1, 120) FROM messages m
                              WHERE m.chat_id = c.id AND m.role = 'user' AND m.deleted_at IS NULL
                              ORDER BY m.id LIMIT 1), '') AS "preview!: String",
                    (SELECT COUNT(*) FROM messages m
                     WHERE m.chat_id = c.id AND m.deleted_at IS NULL) AS "message_count!: i64"
             FROM chats c WHERE c.character_id = ? AND c.deleted_at IS NULL
             ORDER BY c.updated_at DESC, c.id DESC"#,
            character_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The newest chats in which the user wrote at least one message.
    /// 18+ characters stay out until Phase 7 adds the adult content setting.
    pub async fn recent_chats(&self, limit: i64) -> Result<Vec<RecentChat>> {
        let rows = sqlx::query_as!(
            RecentChat,
            r#"SELECT c.id AS "chat_id!", ch.id AS "character_id!", ch.name AS "character_name!",
                    ch.avatar_path, c.updated_at AS "updated_at!",
                    COALESCE((SELECT substr(m.content, 1, 120) FROM messages m
                              WHERE m.id = c.head_message_id AND m.deleted_at IS NULL), '')
                        AS "preview!: String"
             FROM chats c JOIN characters ch ON ch.id = c.character_id
             WHERE c.deleted_at IS NULL AND ch.is_adult = 0
               AND EXISTS (SELECT 1 FROM messages m
                           WHERE m.chat_id = c.id AND m.role = 'user' AND m.deleted_at IS NULL)
             ORDER BY c.updated_at DESC, c.id DESC
             LIMIT ?"#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Creates a chat that starts with the character's greeting.
    pub async fn create_chat(
        &self,
        character: &Character,
        persona_id: Option<i64>,
        title: &str,
        with_greeting: bool,
    ) -> Result<i64> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let chat_id = sqlx::query_scalar!(
            r#"INSERT INTO chats (character_id, persona_id, title, tone, reply_style, reply_custom,
                    created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            character.id,
            persona_id,
            title,
            character.default_tone,
            character.default_reply_style,
            character.default_reply_custom,
            now,
            now
        )
        .fetch_one(&mut *tx)
        .await?;
        if with_greeting && !character.greeting.is_empty() {
            let greeting_id = sqlx::query_scalar!(
                r#"INSERT INTO messages (chat_id, role, content, created_at)
                 VALUES (?, 'character', ?, ?) RETURNING id AS "id!""#,
                chat_id,
                character.greeting,
                now
            )
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE chats SET head_message_id = ? WHERE id = ?",
                greeting_id,
                chat_id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(chat_id)
    }

    pub async fn set_chat_persona(&self, chat_id: i64, persona_id: Option<i64>) -> Result<()> {
        sqlx::query!(
            "UPDATE chats SET persona_id = ? WHERE id = ?",
            persona_id,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_chat_tone(&self, chat_id: i64, tone: &str, custom: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE chats SET tone = ?, tone_custom = ? WHERE id = ?",
            tone,
            custom,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The model for this chat's next replies. `None` uses the character's or the default.
    pub async fn set_chat_model(&self, chat_id: i64, model: Option<&str>) -> Result<()> {
        sqlx::query!(
            "UPDATE chats SET chat_model = ? WHERE id = ?",
            model,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_chat_style(&self, chat_id: i64, style: &str, custom: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE chats SET reply_style = ?, reply_custom = ? WHERE id = ?",
            style,
            custom,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_chat_head(&self, chat_id: i64, head: Option<i64>) -> Result<()> {
        let now = now_ms();
        sqlx::query!(
            "UPDATE chats SET head_message_id = ?, updated_at = ? WHERE id = ?",
            head,
            now,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_chat(&self, chat_id: i64) -> Result<()> {
        let now = now_ms();
        sqlx::query!("UPDATE chats SET deleted_at = ? WHERE id = ?", now, chat_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_context_start(&self, chat_id: i64, message_id: Option<i64>) -> Result<()> {
        sqlx::query!(
            "UPDATE chats SET context_start_id = ? WHERE id = ?",
            message_id,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deleted chats that are not yet gone for good, newest first. 18+
    /// characters stay out, as in `recent_chats`.
    pub async fn deleted_chats(&self) -> Result<Vec<DeletedChat>> {
        let rows = sqlx::query_as!(
            DeletedChat,
            r#"SELECT c.id AS "chat_id!", ch.id AS "character_id!", ch.name AS "character_name!",
                    ch.avatar_path, c.deleted_at AS "deleted_at!",
                    COALESCE((SELECT substr(m.content, 1, 120) FROM messages m
                              WHERE m.id = c.head_message_id AND m.deleted_at IS NULL), '')
                        AS "preview!: String"
             FROM chats c JOIN characters ch ON ch.id = c.character_id
             WHERE c.deleted_at IS NOT NULL AND ch.is_adult = 0
             ORDER BY c.deleted_at DESC, c.id DESC"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn restore_chat(&self, chat_id: i64) -> Result<()> {
        let done = sqlx::query!(
            "UPDATE chats SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL",
            chat_id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("chat"));
        }
        Ok(())
    }

    /// Removes the chats deleted before `before` (ms) for good. Messages,
    /// memories and picture rows go with them. Returns the picture files,
    /// for the caller to delete.
    pub async fn purge_deleted_chats(&self, before: i64) -> Result<Vec<String>> {
        let mut tx = self.pool.begin().await?;
        let files = sqlx::query_scalar!(
            "SELECT i.file_path FROM images i JOIN chats c ON c.id = i.chat_id
             WHERE c.deleted_at < ?",
            before
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM chats WHERE deleted_at < ?", before)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(files)
    }
}
