use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::Serialize;

/// A generated picture. `file_path` is relative to the gallery folder.
#[derive(Debug, Clone, Serialize)]
pub struct Image {
    pub id: i64,
    pub chat_id: i64,
    pub message_id: Option<i64>,
    pub file_path: String,
    pub prompt: String,
    pub negative_prompt: String,
    pub seed: i64,
    pub image_model: String,
    pub ref_mode: String,
    pub width: i64,
    pub height: i64,
    pub adult: bool,
    pub created_at: i64,
}

/// A gallery entry: the picture with its character.
#[derive(Debug, Clone, Serialize)]
pub struct GalleryImage {
    #[serde(flatten)]
    pub image: Image,
    pub character_id: i64,
    pub character_name: String,
}

#[derive(Debug, Clone)]
pub struct NewImage {
    pub chat_id: i64,
    pub message_id: Option<i64>,
    pub file_path: String,
    pub prompt: String,
    pub negative_prompt: String,
    pub seed: i64,
    pub image_model: String,
    pub ref_mode: String,
    pub width: i64,
    pub height: i64,
    pub adult: bool,
}

impl Db {
    pub async fn insert_image(&self, n: &NewImage) -> Result<i64> {
        let now = now_ms();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO images (chat_id, message_id, file_path, prompt, negative_prompt, seed,
                    image_model, ref_mode, width, height, adult, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id AS "id!""#,
            n.chat_id,
            n.message_id,
            n.file_path,
            n.prompt,
            n.negative_prompt,
            n.seed,
            n.image_model,
            n.ref_mode,
            n.width,
            n.height,
            n.adult,
            now
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn image(&self, id: i64) -> Result<Image> {
        sqlx::query_as!(
            Image,
            r#"SELECT id, chat_id, message_id, file_path, prompt, negative_prompt, seed,
                    image_model, ref_mode, width, height, adult AS "adult: bool", created_at
             FROM images WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("picture"))
    }

    /// The pictures of one chat, oldest first, for the chat view.
    pub async fn chat_images(&self, chat_id: i64) -> Result<Vec<Image>> {
        let rows = sqlx::query_as!(
            Image,
            r#"SELECT id AS "id!", chat_id, message_id, file_path, prompt, negative_prompt, seed,
                    image_model, ref_mode, width, height, adult AS "adult: bool", created_at
             FROM images WHERE chat_id = ? ORDER BY created_at, id"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The gallery, newest first. Pictures of deleted chats are left out, and
    /// pictures of 18+ characters unless `adult` (the setting is on).
    pub async fn gallery(&self, adult: bool) -> Result<Vec<GalleryImage>> {
        let rows = sqlx::query!(
            r#"SELECT i.id AS "id!", i.chat_id, i.message_id, i.file_path, i.prompt,
                    i.negative_prompt, i.seed, i.image_model, i.ref_mode, i.width, i.height,
                    i.adult AS "adult: bool", i.created_at,
                    ch.id AS "character_id!", ch.name AS character_name
             FROM images i
             JOIN chats c ON c.id = i.chat_id
             JOIN characters ch ON ch.id = c.character_id
             WHERE c.deleted_at IS NULL AND (ch.is_adult = 0 OR ?)
             ORDER BY i.created_at DESC, i.id DESC"#,
            adult
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| GalleryImage {
                image: Image {
                    id: r.id,
                    chat_id: r.chat_id,
                    message_id: r.message_id,
                    file_path: r.file_path,
                    prompt: r.prompt,
                    negative_prompt: r.negative_prompt,
                    seed: r.seed,
                    image_model: r.image_model,
                    ref_mode: r.ref_mode,
                    width: r.width,
                    height: r.height,
                    adult: r.adult,
                    created_at: r.created_at,
                },
                character_id: r.character_id,
                character_name: r.character_name,
            })
            .collect())
    }

    /// Deletes the row. Returns its file path, so the caller deletes the file.
    pub async fn delete_image(&self, id: i64) -> Result<String> {
        let path = sqlx::query_scalar!("DELETE FROM images WHERE id = ? RETURNING file_path", id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(Error::NotFound("picture"))?;
        Ok(path)
    }

    /// The picture files of a character's chats. Deleting the character
    /// removes the rows, so the caller reads these first to delete the files.
    pub async fn character_image_files(&self, character_id: i64) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar!(
            "SELECT i.file_path FROM images i JOIN chats c ON c.id = i.chat_id
             WHERE c.character_id = ?",
            character_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
