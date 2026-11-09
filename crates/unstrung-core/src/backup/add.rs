//! "Add to my data": copies the characters and personas of a backup.

use super::*;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Added {
    pub characters: usize,
    pub personas: usize,
    /// Already here with the same text, so not added again (the starters).
    pub skipped: usize,
}

/// Copies a staged avatar (a clean PNG already, encrypted) into the avatars folder
/// under a new name. `None` when the backup has no such file.
pub(super) fn copy_avatar(
    paths: &AppPaths,
    staged: &Path,
    stored: Option<&str>,
    n: usize,
) -> Result<Option<String>> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    let Ok(bytes) = std::fs::read(join_safe(staged, stored)?) else {
        return Ok(None);
    };
    let name = format!("avatars/imported-{}-{n}.png", now_ms());
    crate::files::write_atomic(&paths.stored(&name)?, &bytes)?;
    Ok(Some(name))
}

pub(super) async fn add_rows(
    staged: &Db,
    live: &Db,
    paths: &AppPaths,
    dir: &Path,
    added: &mut (Vec<i64>, Vec<i64>),
) -> Result<Added> {
    let keep = crate::avatar::AvatarChange::Keep;
    let mut done = Added::default();
    let have = live.characters().await?;
    for (n, c) in staged.characters().await?.into_iter().enumerate() {
        let same = have.iter().any(|h| {
            h.name == c.name && h.personality == c.personality && h.greeting == c.greeting
        });
        if same {
            done.skipped += 1;
            continue;
        }
        let extras = staged.character_extras(c.id).await?;
        // The creator's checks again: the live data must never hold a bad row.
        let saved = crate::library::save_character(
            live,
            paths,
            None,
            &character_fields(&c),
            &keep,
            Some(&extras),
        )
        .await?;
        added.0.push(saved.id);
        if let Some(avatar) = copy_avatar(paths, dir, c.avatar_path.as_deref(), n)? {
            live.set_character_avatar(saved.id, Some(&avatar)).await?;
        }
        let kept = staged.kept(c.id).await?;
        live.set_memory_mode(saved.id, &kept.mode).await?;
        for m in kept.memories {
            if let Some(note) = crate::memory::user_note(&m.content, "fact") {
                live.keep_memory(saved.id, &note).await?;
            }
        }
        done.characters += 1;
    }
    let have = live.personas().await?;
    for (n, p) in staged.personas().await?.into_iter().enumerate() {
        if have
            .iter()
            .any(|h| h.name == p.name && h.description == p.description)
        {
            done.skipped += 1;
            continue;
        }
        let saved =
            crate::library::save_persona(live, paths, None, &persona_fields(&p), &keep).await?;
        added.1.push(saved.id);
        if let Some(avatar) = copy_avatar(paths, dir, p.avatar_path.as_deref(), n + 100_000)? {
            live.set_persona_avatar(saved.id, Some(&avatar)).await?;
        }
        done.personas += 1;
    }
    Ok(done)
}

/// "Add characters only": adds the characters and personas of an opened
/// backup, with their pictures, world info and kept facts, as new ones.
/// Chats and the gallery stay out. One that is already here with the same
/// text is skipped. A failure removes what this call added.
pub async fn add_characters(live: &Db, paths: &AppPaths, id: &str) -> Result<Added> {
    let (dir, _) = staging(paths, id)?;
    let file = dir.join(DATABASE);
    if !file.exists() {
        return Err(Error::Invalid("this backup is no longer open".into()));
    }
    let staged = Db::open(&file, &paths.key()?).await?;
    let mut added = (Vec::new(), Vec::new());
    let result = add_rows(&staged, live, paths, &dir, &mut added).await;
    staged.close().await;
    if result.is_err() {
        for character in added.0 {
            if let Err(e) = crate::library::delete_character(live, paths, character).await {
                log::warn!("could not remove a half-added character: {e}");
            }
        }
        for persona in added.1 {
            if let Err(e) = crate::library::delete_persona(live, paths, persona).await {
                log::warn!("could not remove a half-added persona: {e}");
            }
        }
    } else {
        discard(paths, id);
    }
    result
}
