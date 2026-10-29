use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Character {
    pub id: i64,
    pub name: String,
    pub avatar_path: Option<String>,
    pub tagline: String,
    pub personality: String,
    pub scenario: String,
    pub greeting: String,
    pub example_dialogue: String,
    pub voice_anchor: String,
    pub appearance_tags: String,
    pub default_tone: String,
    pub default_reply_style: String,
    /// StyleSettings JSON when `default_reply_style` is "custom", else empty.
    pub default_reply_custom: String,
    pub model_override: Option<String>,
    pub age: i64,
    pub is_adult: bool,
    pub created_at: i64,
}

/// The fields that the creator and the card import set. Checked by `characters::validate`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterFields {
    pub name: String,
    pub tagline: String,
    pub personality: String,
    pub scenario: String,
    pub greeting: String,
    pub example_dialogue: String,
    pub voice_anchor: String,
    pub appearance_tags: String,
    pub default_tone: String,
    pub default_reply_style: String,
    #[serde(default)]
    pub default_reply_custom: String,
    pub age: i64,
    pub is_adult: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldEntry {
    pub keywords: String,
    pub content: String,
    pub is_enabled: bool,
}

impl Db {
    pub async fn character(&self, id: i64) -> Result<Character> {
        sqlx::query_as!(
            Character,
            r#"SELECT id, name, avatar_path, tagline, personality, scenario, greeting,
                    example_dialogue, voice_anchor, appearance_tags, default_tone,
                    default_reply_style, default_reply_custom, model_override, age, is_adult AS "is_adult: bool",
                    created_at
             FROM characters WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("character"))
    }

    pub async fn characters(&self) -> Result<Vec<Character>> {
        let rows = sqlx::query_as!(
            Character,
            r#"SELECT id AS "id!", name, avatar_path, tagline, personality, scenario, greeting,
                    example_dialogue, voice_anchor, appearance_tags, default_tone,
                    default_reply_style, default_reply_custom, model_override, age, is_adult AS "is_adult: bool",
                    created_at
             FROM characters ORDER BY created_at, id"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn character_by_starter(&self, key: &str) -> Result<Option<i64>> {
        let id = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM characters WHERE starter_key = ?"#,
            key
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn insert_character(
        &self,
        f: &CharacterFields,
        starter_key: Option<&str>,
        extensions_json: &str,
    ) -> Result<i64> {
        let now = now_ms();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO characters (starter_key, name, tagline, personality, scenario,
                    greeting, example_dialogue, voice_anchor, appearance_tags, default_tone,
                    default_reply_style, default_reply_custom, age, is_adult, extensions_json,
                    created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id AS "id!""#,
            starter_key,
            f.name,
            f.tagline,
            f.personality,
            f.scenario,
            f.greeting,
            f.example_dialogue,
            f.voice_anchor,
            f.appearance_tags,
            f.default_tone,
            f.default_reply_style,
            f.default_reply_custom,
            f.age,
            f.is_adult,
            extensions_json,
            now
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_character(&self, id: i64, f: &CharacterFields) -> Result<()> {
        let done = sqlx::query!(
            "UPDATE characters SET name = ?, tagline = ?, personality = ?, scenario = ?,
                    greeting = ?, example_dialogue = ?, voice_anchor = ?, appearance_tags = ?,
                    default_tone = ?, default_reply_style = ?, default_reply_custom = ?,
                    age = ?, is_adult = ?
             WHERE id = ?",
            f.name,
            f.tagline,
            f.personality,
            f.scenario,
            f.greeting,
            f.example_dialogue,
            f.voice_anchor,
            f.appearance_tags,
            f.default_tone,
            f.default_reply_style,
            f.default_reply_custom,
            f.age,
            f.is_adult,
            id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("character"));
        }
        Ok(())
    }

    pub async fn set_character_avatar(&self, id: i64, path: Option<&str>) -> Result<()> {
        sqlx::query!(
            "UPDATE characters SET avatar_path = ? WHERE id = ?",
            path,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_voice_anchor(&self, id: i64, anchor: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE characters SET voice_anchor = ? WHERE id = ?",
            anchor,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes the character with its chats, messages and world info.
    pub async fn delete_character(&self, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM characters WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_world_entries(&self, character_id: i64, entries: &[WorldEntry]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for e in entries {
            sqlx::query!(
                "INSERT INTO world_info (character_id, keywords, content, is_enabled)
                 VALUES (?, ?, ?, ?)",
                character_id,
                e.keywords,
                e.content,
                e.is_enabled
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn world_entry_count(&self, character_id: i64) -> Result<i64> {
        let n = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM world_info WHERE character_id = ?",
            character_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }
}
