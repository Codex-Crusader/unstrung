//! SQLite storage. One connection, so the core is the only writer.

use crate::error::{Error, Result};
use serde::Serialize;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

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
    pub default_tone: String,
    pub default_reply_style: String,
    pub model_override: Option<String>,
    pub age: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chat {
    pub id: i64,
    pub character_id: i64,
    pub title: String,
    pub tone: String,
    pub reply_style: String,
    pub chat_model: Option<String>,
    pub head_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: i64,
}

/// A message that is not saved yet.
#[derive(Debug, Default)]
pub struct NewMessage<'a> {
    pub chat_id: i64,
    pub parent_id: Option<i64>,
    pub role: &'a str,
    pub content: &'a str,
    pub raw_content: Option<&'a str>,
    pub token_count: Option<i64>,
    pub model: Option<&'a str>,
    pub prompt_version: Option<i64>,
    pub sampler_json: Option<&'a str>,
}

/// Values for a new character record.
pub struct NewCharacter<'a> {
    pub starter_key: Option<&'a str>,
    pub name: &'a str,
    pub avatar_path: Option<&'a str>,
    pub tagline: &'a str,
    pub personality: &'a str,
    pub scenario: &'a str,
    pub greeting: &'a str,
    pub example_dialogue: &'a str,
    pub voice_anchor: &'a str,
    pub appearance_tags: &'a str,
    pub default_tone: &'a str,
    pub default_reply_style: &'a str,
    pub age: i64,
}

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens (or creates) the database and runs the migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        let db = Self { pool };
        let schema = MIGRATOR.iter().map(|m| m.version).max().unwrap_or_default();
        db.set_meta("schema_version", &schema.to_string()).await?;
        db.set_meta("app_version", env!("CARGO_PKG_VERSION"))
            .await?;
        db.set_meta("prompt_version", &crate::prompt::PROMPT_VERSION.to_string())
            .await?;
        Ok(db)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn quick_check(&self) -> Result<bool> {
        let row: (String,) = sqlx::query_as("PRAGMA quick_check")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 == "ok")
    }

    pub async fn meta(&self, key: &str) -> Result<Option<String>> {
        let value = sqlx::query_scalar!("SELECT value FROM meta WHERE key = ?", key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(value)
    }

    pub async fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query!(
            "INSERT INTO meta (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn character(&self, id: i64) -> Result<Character> {
        sqlx::query_as!(
            Character,
            "SELECT id, name, avatar_path, tagline, personality, scenario, greeting,
                    example_dialogue, voice_anchor, default_tone, default_reply_style,
                    model_override, age
             FROM characters WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("character"))
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

    pub async fn insert_character(&self, c: &NewCharacter<'_>) -> Result<i64> {
        let now = now_ms();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO characters (starter_key, name, avatar_path, tagline, personality,
                    scenario, greeting, example_dialogue, voice_anchor, appearance_tags,
                    default_tone, default_reply_style, age, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id AS "id!""#,
            c.starter_key,
            c.name,
            c.avatar_path,
            c.tagline,
            c.personality,
            c.scenario,
            c.greeting,
            c.example_dialogue,
            c.voice_anchor,
            c.appearance_tags,
            c.default_tone,
            c.default_reply_style,
            c.age,
            now
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn chat(&self, id: i64) -> Result<Chat> {
        sqlx::query_as!(
            Chat,
            "SELECT id, character_id, title, tone, reply_style, chat_model, head_message_id
             FROM chats WHERE id = ? AND deleted_at IS NULL",
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("chat"))
    }

    /// The most recently used chat with a character.
    pub async fn latest_chat(&self, character_id: i64) -> Result<Option<Chat>> {
        let chat = sqlx::query_as!(
            Chat,
            r#"SELECT id AS "id!", character_id, title, tone, reply_style, chat_model, head_message_id
             FROM chats WHERE character_id = ? AND deleted_at IS NULL
             ORDER BY updated_at DESC, id DESC LIMIT 1"#,
            character_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(chat)
    }

    /// Creates a chat that starts with the character's greeting.
    pub async fn create_chat(&self, character: &Character) -> Result<Chat> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let chat_id = sqlx::query_scalar!(
            r#"INSERT INTO chats (character_id, tone, reply_style, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            character.id,
            character.default_tone,
            character.default_reply_style,
            now,
            now
        )
        .fetch_one(&mut *tx)
        .await?;
        if !character.greeting.is_empty() {
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
        self.chat(chat_id).await
    }

    /// Saves a message under its parent and makes it the new head of the chat.
    pub async fn append_message(&self, m: &NewMessage<'_>) -> Result<Message> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO messages (chat_id, parent_id, role, content, raw_content, token_count,
                    model, prompt_version, sampler_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            m.chat_id,
            m.parent_id,
            m.role,
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
            content: m.content.to_owned(),
            model: m.model.map(str::to_owned),
            created_at: now,
        })
    }

    /// The messages from the first message to the head, oldest first.
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
                    m.content AS "content!", m.model, m.created_at AS "created_at!"
             FROM path JOIN messages m ON m.id = path.id AND m.chat_id = ?1
             ORDER BY path.depth DESC"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ren() -> NewCharacter<'static> {
        NewCharacter {
            starter_key: Some("test"),
            name: "Ren",
            avatar_path: None,
            tagline: "",
            personality: "Tired.",
            scenario: "",
            greeting: "Sit. Talk.",
            example_dialogue: "",
            voice_anchor: "",
            appearance_tags: "",
            default_tone: "dark",
            default_reply_style: "conversational",
            age: 34,
        }
    }

    #[tokio::test]
    async fn message_tree_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.db");
        let db = Db::open(&file).await.unwrap();
        assert!(db.quick_check().await.unwrap());
        let id = db.insert_character(&ren()).await.unwrap();
        let character = db.character(id).await.unwrap();
        let chat = db.create_chat(&character).await.unwrap();
        let greeting = chat.head_message_id.unwrap();
        let user = db
            .append_message(&NewMessage {
                chat_id: chat.id,
                parent_id: Some(greeting),
                role: "user",
                content: "Hello",
                ..Default::default()
            })
            .await
            .unwrap();
        // A sibling branch that is not on the path to the head.
        db.append_message(&NewMessage {
            chat_id: chat.id,
            parent_id: Some(greeting),
            role: "user",
            content: "Other branch",
            ..Default::default()
        })
        .await
        .unwrap();
        db.append_message(&NewMessage {
            chat_id: chat.id,
            parent_id: Some(user.id),
            role: "character",
            content: "Talk.",
            ..Default::default()
        })
        .await
        .unwrap();
        db.close().await;

        let db = Db::open(&file).await.unwrap();
        let path = db.path_to_head(chat.id).await.unwrap();
        let texts: Vec<&str> = path.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(texts, ["Sit. Talk.", "Hello", "Talk."]);
        assert_eq!(db.latest_chat(id).await.unwrap().unwrap().id, chat.id);
        assert_eq!(
            db.meta("schema_version").await.unwrap().as_deref(),
            Some("1")
        );
    }
}
