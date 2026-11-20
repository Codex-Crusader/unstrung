use super::{Db, now_ms};
use crate::error::Result;
use crate::growth::{Appraisal, Genome, State};

impl Db {
    /// The genome of a character and the hash of the card it was made from.
    pub async fn genome(&self, character_id: i64) -> Result<Option<(String, Genome)>> {
        let row = sqlx::query!(
            "SELECT card_hash, genome FROM character_genomes WHERE character_id = ?",
            character_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            Some(r) => Some((r.card_hash, serde_json::from_str(&r.genome)?)),
            None => None,
        })
    }

    /// Saves the genome of a character, in place of an older one.
    pub async fn set_genome(
        &self,
        character_id: i64,
        card_hash: &str,
        genome: &Genome,
    ) -> Result<()> {
        let json = serde_json::to_string(genome)?;
        let now = now_ms();
        sqlx::query!(
            "INSERT INTO character_genomes (character_id, card_hash, genome, created_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (character_id) DO UPDATE
             SET card_hash = excluded.card_hash, genome = excluded.genome, created_at = excluded.created_at",
            character_id,
            card_hash,
            json,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Saves how the character feels after a reply. False when the reply
    /// has a state already: the first appraisal of a reply wins, so a retry
    /// never applies the same change twice.
    pub async fn add_growth_state(
        &self,
        chat_id: i64,
        message_id: i64,
        state: &State,
        appraisal: &Appraisal,
    ) -> Result<bool> {
        let state = serde_json::to_string(state)?;
        let appraisal = serde_json::to_string(appraisal)?;
        let now = now_ms();
        let done = sqlx::query!(
            "INSERT INTO growth_states (message_id, chat_id, state, appraisal, created_at)
             VALUES (?, ?, ?, ?, ?) ON CONFLICT (message_id) DO NOTHING",
            message_id,
            chat_id,
            state,
            appraisal,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Every growth state of a chat with its reply and appraisal, oldest first.
    // shortcut: reads every row of the chat (about 3 MB of JSON at the
    // 20,000-message import limit). Read only the path if replies slow down.
    pub async fn growth_states(&self, chat_id: i64) -> Result<Vec<(i64, State, Appraisal)>> {
        let rows = sqlx::query!(
            r#"SELECT message_id AS "message_id!", state, appraisal
             FROM growth_states WHERE chat_id = ? ORDER BY message_id"#,
            chat_id
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok((
                    r.message_id,
                    serde_json::from_str(&r.state)?,
                    serde_json::from_str(&r.appraisal)?,
                ))
            })
            .collect()
    }
}
