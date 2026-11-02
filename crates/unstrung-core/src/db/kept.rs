//! Character memories: facts that a character keeps for new chats.

use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CharacterMemory {
    pub id: i64,
    pub content: String,
    pub created_at: i64,
}

/// What the memory panel shows for a character.
#[derive(Debug, Clone, Serialize)]
pub struct Kept {
    /// Whether new chats use the memories: "off", "ask" or "always".
    pub mode: String,
    pub memories: Vec<CharacterMemory>,
}

pub const MEMORY_MODES: [&str; 3] = ["off", "ask", "always"];

impl Db {
    pub async fn kept(&self, character_id: i64) -> Result<Kept> {
        let mode = sqlx::query_scalar!(
            "SELECT memory_mode FROM characters WHERE id = ?",
            character_id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("character"))?;
        let memories = sqlx::query_as!(
            CharacterMemory,
            r#"SELECT id AS "id!", content, created_at FROM character_memories
             WHERE character_id = ? ORDER BY id"#,
            character_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(Kept { mode, memories })
    }

    /// Keeps a fact for new chats. The same text twice stays one memory.
    pub async fn keep_memory(&self, character_id: i64, content: &str) -> Result<()> {
        let content = content.trim();
        if content.is_empty() {
            return Err(Error::Invalid("the fact is empty".into()));
        }
        let now = now_ms();
        sqlx::query!(
            "INSERT OR IGNORE INTO character_memories (character_id, content, created_at)
             VALUES (?, ?, ?)",
            character_id,
            content,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn forget_character_memory(&self, character_id: i64, id: i64) -> Result<()> {
        let done = sqlx::query!(
            "DELETE FROM character_memories WHERE id = ? AND character_id = ?",
            id,
            character_id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("memory"));
        }
        Ok(())
    }

    pub async fn set_memory_mode(&self, character_id: i64, mode: &str) -> Result<()> {
        if !MEMORY_MODES.contains(&mode) {
            return Err(Error::Invalid(format!("unknown memory setting {mode}")));
        }
        let done = sqlx::query!(
            "UPDATE characters SET memory_mode = ? WHERE id = ?",
            mode,
            character_id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("character"));
        }
        Ok(())
    }

    pub async fn set_chat_use_memory(&self, chat_id: i64, on: bool) -> Result<()> {
        let done = sqlx::query!(
            "UPDATE chats SET use_character_memory = ? WHERE id = ? AND deleted_at IS NULL",
            on,
            chat_id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("chat"));
        }
        Ok(())
    }
}
