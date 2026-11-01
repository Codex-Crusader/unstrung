use super::{Db, now_ms};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Persona {
    pub id: i64,
    pub name: String,
    pub avatar_path: Option<String>,
    pub description: String,
    pub appearance_tags: String,
    /// What the persona usually wears. A picture can show other clothes.
    #[serde(default)]
    pub outfit: String,
    pub age: Option<i64>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonaFields {
    pub name: String,
    pub description: String,
    pub appearance_tags: String,
    /// What the persona usually wears. A picture can show other clothes.
    #[serde(default)]
    pub outfit: String,
    pub age: Option<i64>,
}

impl Db {
    pub async fn persona(&self, id: i64) -> Result<Persona> {
        sqlx::query_as!(
            Persona,
            r#"SELECT id, name, avatar_path, description, appearance_tags, outfit, age,
                    is_default AS "is_default: bool"
             FROM personas WHERE id = ?"#,
            id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound("persona"))
    }

    pub async fn personas(&self) -> Result<Vec<Persona>> {
        let rows = sqlx::query_as!(
            Persona,
            r#"SELECT id AS "id!", name, avatar_path, description, appearance_tags, outfit, age,
                    is_default AS "is_default: bool"
             FROM personas ORDER BY is_default DESC, name COLLATE NOCASE, id"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn default_persona(&self) -> Result<Option<i64>> {
        let id =
            sqlx::query_scalar!(r#"SELECT id AS "id!" FROM personas WHERE is_default = 1 LIMIT 1"#)
                .fetch_optional(&self.pool)
                .await?;
        Ok(id)
    }

    pub async fn persona_by_starter(&self, key: &str) -> Result<Option<i64>> {
        let id = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM personas WHERE starter_key = ?"#,
            key
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn insert_persona(
        &self,
        f: &PersonaFields,
        starter_key: Option<&str>,
    ) -> Result<i64> {
        let now = now_ms();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO personas (starter_key, name, description, appearance_tags, outfit, age, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            starter_key,
            f.name,
            f.description,
            f.appearance_tags,
            f.outfit,
            f.age,
            now
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn update_persona(&self, id: i64, f: &PersonaFields) -> Result<()> {
        let done = sqlx::query!(
            "UPDATE personas SET name = ?, description = ?, appearance_tags = ?, outfit = ?, age = ?
             WHERE id = ?",
            f.name,
            f.description,
            f.appearance_tags,
            f.outfit,
            f.age,
            id
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(Error::NotFound("persona"));
        }
        Ok(())
    }

    pub async fn set_persona_avatar(&self, id: i64, path: Option<&str>) -> Result<()> {
        sqlx::query!("UPDATE personas SET avatar_path = ? WHERE id = ?", path, id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Makes one persona the default, or clears the default.
    pub async fn set_default_persona(&self, id: Option<i64>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("UPDATE personas SET is_default = 0")
            .execute(&mut *tx)
            .await?;
        if let Some(id) = id {
            sqlx::query!("UPDATE personas SET is_default = 1 WHERE id = ?", id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Chats and messages keep working: their persona becomes empty.
    pub async fn delete_persona(&self, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM personas WHERE id = ?", id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
