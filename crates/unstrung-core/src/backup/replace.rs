//! "Replace everything": the next start swaps the data for a backup.

use super::*;

/// The note that asks the next start to replace the data with a backup:
/// `<import id> <time>`.
pub(super) const PENDING: &str = "pending-import.txt";

pub(super) fn digits(text: &str) -> bool {
    !text.is_empty() && text.len() <= 40 && text.bytes().all(|b| b.is_ascii_digit())
}

/// "Replace": the next start swaps the data for the opened backup
/// (`apply_pending`). The app must restart now, so nothing holds the files.
pub fn ask_replace(paths: &AppPaths, id: &str) -> Result<()> {
    let (staged, _) = staging(paths, id)?;
    if !staged.join(DATABASE).exists() {
        return Err(Error::Invalid("this backup is no longer open".into()));
    }
    let note = format!("{id} {}", now_ms());
    crate::files::write_atomic(&paths.backups().join(PENDING), note.as_bytes())
}

/// Takes back `ask_replace` when the restart did not happen, so no later
/// start swaps the data without the user knowing.
pub fn cancel_replace(paths: &AppPaths) {
    let note = paths.backups().join(PENDING);
    if let Err(e) = std::fs::remove_file(&note)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::error!("could not remove the import note: {e}");
    }
}

/// The moves of a Replace in order: the current data aside into
/// `replaced-import-<time>`, then the staged backup in. Paths only from
/// fixed names and the two checked numbers.
pub(super) fn replace_moves(
    paths: &AppPaths,
    id: &str,
    stamp: &str,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    if !digits(stamp) {
        return Err(Error::Invalid("the import note is damaged".into()));
    }
    let (staged, staged_gallery) = staging(paths, id)?;
    let aside = paths.backups().join(format!("replaced-import-{stamp}"));
    let db = paths.database();
    let journal = |base: &Path, suffix: &str| PathBuf::from(format!("{}{suffix}", base.display()));
    let mut moves = Vec::new();
    // The chats file with its journal files, or none of them.
    for suffix in ["", "-wal", "-shm"] {
        moves.push((journal(&db, suffix), journal(&aside.join(DATABASE), suffix)));
    }
    moves.push((paths.avatars(), aside.join("avatars")));
    moves.push((
        paths.gallery(),
        paths.library().join(format!("gallery-replaced-{stamp}")),
    ));
    for suffix in ["", "-wal", "-shm"] {
        moves.push((
            journal(&staged.join(DATABASE), suffix),
            journal(&db, suffix),
        ));
    }
    moves.push((staged.join("avatars"), paths.avatars()));
    moves.push((staged_gallery, paths.gallery()));
    Ok(moves)
}

/// Moves back what `moves` moved, newest first. Safe to run on a
/// half-done Replace: it moves only what is at the target and not at the source.
pub(super) fn undo(moves: &[(PathBuf, PathBuf)]) {
    for (from, to) in moves.iter().rev() {
        if to.exists()
            && !from.exists()
            && let Err(e) = std::fs::rename(to, from)
        {
            log::error!("could not move {} back: {e}", to.display());
        }
    }
}

/// Runs a Replace that the last run asked for, before the chats file
/// opens. `Ok(false)`: none asked. On a failure, and after a crash part
/// way, everything moves back and the error says so. The data before the
/// Replace stays in `backups/replaced-import-<time>`.
pub fn apply_pending(paths: &AppPaths) -> Result<bool> {
    let note_file = paths.backups().join(PENDING);
    let Ok(note) = std::fs::read_to_string(&note_file) else {
        return Ok(false);
    };
    let give_up = |e: Error| {
        if let Err(r) = std::fs::remove_file(&note_file) {
            log::error!("could not remove the import note: {r}");
        }
        e
    };
    let mut parts = note.split_whitespace();
    let (Some(id), Some(stamp)) = (parts.next(), parts.next()) else {
        return Err(give_up(Error::Invalid("the import note is damaged".into())));
    };
    let moves = replace_moves(paths, id, stamp).map_err(give_up)?;
    let aside = paths.backups().join(format!("replaced-import-{stamp}"));
    if aside.exists() {
        // An earlier start stopped part way (a crash or a power cut).
        undo(&moves);
        return Err(give_up(Error::Invalid(
            "the backup import stopped part way. Your data is back as it was before".into(),
        )));
    }
    let (staged, _) = staging(paths, id).map_err(give_up)?;
    if !staged.join(DATABASE).exists() {
        return Err(give_up(Error::Invalid(
            "the backup to import is gone".into(),
        )));
    }
    let moved = (|| -> Result<()> {
        std::fs::create_dir_all(&aside)?;
        for (i, (from, to)) in moves.iter().enumerate() {
            if !from.exists() {
                continue;
            }
            // A rename on Windows can replace what is at the target: never let it.
            let moved = if to.exists() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} is in the way", to.display()),
                ))
            } else {
                std::fs::rename(from, to)
            };
            if let Err(e) = moved {
                undo(&moves[..i]);
                return Err(e.into());
            }
        }
        // While the note is there, the next start undoes the Replace.
        if let Err(e) = std::fs::remove_file(&note_file) {
            undo(&moves);
            return Err(e.into());
        }
        Ok(())
    })();
    match moved {
        Ok(()) => {
            discard(paths, id);
            log::info!(
                "the data is replaced by the backup; the old data is in {}",
                aside.display()
            );
            Ok(true)
        }
        Err(e) => Err(give_up(Error::Invalid(format!(
            "the backup could not replace your data ({e}). Your data is as it was before"
        )))),
    }
}
