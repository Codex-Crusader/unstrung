//! Settings > Storage > Change location: moves the pictures or the models
//! to another folder. Only the app's own subfolders move (`gallery`, or
//! `ollama` and `image`), never a whole folder, because the library and the
//! models folder can be the same folder or hold the user's own files.
//!
//! Order: copy each subfolder to `<new>.moving`, check the sizes, rename it
//! to `<new>`, save `location.json`, then delete the old subfolders. A note
//! in `backups` lists the moves, so a start after a crash either finishes
//! the delete (the location already changed) or removes the copies (it did
//! not). The old files stay until the new location is in use.

use crate::error::{Error, Result};
use crate::location::{self, Location, Purpose};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const NOTE: &str = "move.json";
const MOVING: &str = ".moving";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// (old subfolder, new subfolder)
    pub moves: Vec<(PathBuf, PathBuf)>,
    /// The location to save when the copies are in place.
    pub location: Location,
    /// Bytes to copy.
    pub bytes: u64,
    /// For a library move: mark the new folder, so a reinstall finds it.
    pub mark: Option<PathBuf>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

/// `inner` is `outer` or a folder inside it (Windows paths: no case).
fn inside(inner: &Path, outer: &Path) -> bool {
    let (i, o) = (key(inner), key(outer));
    i == o || i.starts_with(&format!("{o}\\"))
}

fn moving(new: &Path) -> PathBuf {
    let mut name = new.as_os_str().to_owned();
    name.push(MOVING);
    PathBuf::from(name)
}

fn empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut e| e.next().is_none())
}

/// Checks a move of the pictures (`Purpose::Library`) or the models to the
/// folder the user picked. Changes nothing.
pub fn plan(paths: &AppPaths, purpose: Purpose, picked: &Path) -> Result<Plan> {
    let root = paths.root();
    let now = paths.location();
    let (old_folder, subfolders): (PathBuf, &[&str]) = match purpose {
        Purpose::Library => (now.library(root), &["gallery"]),
        Purpose::Models => (now.models(root), &["ollama", "image"]),
    };
    let olds: Vec<PathBuf> = subfolders
        .iter()
        .map(|s| old_folder.join(s))
        .filter(|p| p.is_dir())
        .collect();
    let bytes = olds.iter().map(|p| crate::repair::folder_size(p)).sum();
    let check = location::check(picked, purpose, bytes);
    if check.blocked {
        return Err(Error::Invalid(format!(
            "this folder cannot be used: {:?}",
            check.issues
        )));
    }
    // Back to the app folder: it is never empty, but it is the default.
    let folder = if key(picked) == key(root) {
        root.to_path_buf()
    } else {
        check.folder
    };
    if key(&folder) == key(&old_folder) {
        return Err(Error::Invalid("this is the folder in use now".into()));
    }
    let mut moves = Vec::new();
    for old in olds {
        let new = folder.join(old.file_name().unwrap_or_default());
        if inside(&new, &old) {
            return Err(Error::Invalid(
                "the new folder cannot be inside the old one".into(),
            ));
        }
        if new.exists() && !empty_dir(&new) {
            return Err(Error::Invalid(format!(
                "{} already has files. Pick another folder",
                new.display()
            )));
        }
        moves.push((old, new));
    }
    let location = match purpose {
        // The models stay where they are: without a saved models folder,
        // they would follow the library and look missing.
        Purpose::Library => Location {
            library: Some(folder.clone()),
            models: Some(now.models(root)),
        },
        Purpose::Models => Location {
            library: now.library,
            models: Some(folder.clone()),
        },
    };
    Ok(Plan {
        moves,
        location,
        bytes,
        mark: (purpose == Purpose::Library).then_some(folder),
    })
}

/// Copies `from` into `to`, file by file. Links are skipped, as in export.
/// shortcut: a stop waits for the file in copy (a model file can be 5 GB);
/// copy in chunks if that wait is a problem.
fn copy_tree(
    from: &Path,
    to: &Path,
    done: &mut u64,
    progress: &mut dyn FnMut(u64),
    cancel: &CancellationToken,
) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, done, progress, cancel)?;
        } else if kind.is_file() {
            let size = entry.metadata()?.len();
            let copied = std::fs::copy(entry.path(), &target)?;
            // The check of the copy: every byte arrived.
            if copied != size || std::fs::metadata(&target)?.len() != size {
                return Err(Error::Invalid(format!(
                    "the copy of {} is not complete",
                    entry.path().display()
                )));
            }
            *done += size;
            progress(*done);
        } else {
            log::warn!("not moved (a link): {}", entry.path().display());
        }
    }
    Ok(())
}

fn remove(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("could not delete {}: {e}", dir.display());
    }
}

/// Removes the copies of a move that did not finish.
fn undo(plan: &Plan) {
    for (_, new) in &plan.moves {
        remove(&moving(new));
        remove(new);
    }
}

/// The new location is in use: the old subfolders go.
fn finish(paths: &AppPaths, plan: &Plan, old_library: &Path) {
    for (old, new) in &plan.moves {
        if new.is_dir() {
            remove(old);
        }
    }
    if plan.mark.is_some() && key(old_library) != key(paths.root()) {
        let _ = std::fs::remove_file(old_library.join(location::LIBRARY_MARK));
    }
    // The old models folder itself, only when nothing else is in it.
    if let Some((old, _)) = plan.moves.first()
        && let Some(parent) = old.parent()
        && plan.mark.is_none()
        && key(parent) != key(paths.root())
    {
        let _ = std::fs::remove_dir(parent);
    }
}

/// Does the move. Blocking: run it off the async threads, with the engines
/// stopped and no job or download running. `progress` gets the bytes copied.
pub fn run(
    paths: &AppPaths,
    plan: &Plan,
    progress: &mut dyn FnMut(u64),
    cancel: &CancellationToken,
) -> Result<()> {
    let note = paths.backups().join(NOTE);
    crate::files::write_atomic(&note, &serde_json::to_vec_pretty(plan)?)?;
    let old_library = paths.library();
    let mut done = 0;
    let copied = (|| {
        for (old, new) in &plan.moves {
            if new.exists() {
                std::fs::remove_dir(new)?;
            }
            let tmp = moving(new);
            remove(&tmp);
            copy_tree(old, &tmp, &mut done, progress, cancel)?;
        }
        for (_, new) in &plan.moves {
            std::fs::rename(moving(new), new)?;
        }
        if let Some(folder) = &plan.mark {
            location::mark_library(folder)?;
        }
        paths.set_location(plan.location.clone())
    })();
    if let Err(e) = copied {
        undo(plan);
        let _ = std::fs::remove_file(&note);
        return Err(e);
    }
    log::info!("moved {} subfolders, {done} bytes", plan.moves.len());
    finish(paths, plan, &old_library);
    let _ = std::fs::remove_file(&note);
    Ok(())
}

/// At start, before anything reads the folders: ends a move that a crash
/// stopped. `old_library` is read from the note's moves.
pub fn recover(paths: &AppPaths) {
    let note = paths.backups().join(NOTE);
    let Ok(bytes) = std::fs::read(&note) else {
        return;
    };
    match serde_json::from_slice::<Plan>(&bytes) {
        Ok(plan) if paths.location() == plan.location => {
            log::warn!("a move stopped after the switch: the old folders go now");
            let old_library = plan
                .moves
                .first()
                .and_then(|(old, _)| old.parent())
                .map_or_else(|| paths.root().to_path_buf(), Path::to_path_buf);
            finish(paths, &plan, &old_library);
        }
        Ok(plan) => {
            log::warn!("a move stopped before the switch: the copies go");
            undo(&plan);
        }
        Err(e) => log::error!("the move note is damaged, so it is left alone: {e}"),
    }
    let _ = std::fs::remove_file(&note);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().join("home"));
        paths.ensure().unwrap();
        let write = |p: PathBuf| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, [1u8; 10]).unwrap();
        };
        write(paths.gallery().join("1").join("p.png"));
        write(paths.ollama_models().join("blobs").join("sha256-a"));
        write(paths.image_models().join("m.gguf"));
        (dir, paths)
    }

    #[test]
    fn a_library_move_keeps_the_models_where_they_are() {
        let (dir, paths) = home();
        let models = paths.models();
        let to = dir.path().join("pics");
        let plan = plan(&paths, Purpose::Library, &to).unwrap();
        assert_eq!(plan.bytes, 10);
        let mut last = 0;
        run(&paths, &plan, &mut |n| last = n, &CancellationToken::new()).unwrap();
        assert_eq!(last, 10);
        assert_eq!(paths.models(), models, "the models path did not change");
        assert!(paths.image_models().join("m.gguf").exists());
        assert_eq!(paths.gallery(), to.join("gallery"));
        assert!(paths.gallery().join("1").join("p.png").exists());
        assert!(to.join(location::LIBRARY_MARK).exists());
        assert!(!paths.root().join("gallery").exists(), "the old copy went");
        assert!(!paths.backups().join(NOTE).exists());
        // The same folder again is refused.
        assert!(super::plan(&paths, Purpose::Library, &to).is_err());

        // And back to the app folder.
        let back = super::plan(&paths, Purpose::Library, paths.root()).unwrap();
        run(&paths, &back, &mut |_| {}, &CancellationToken::new()).unwrap();
        assert_eq!(paths.gallery(), paths.root().join("gallery"));
        assert!(paths.gallery().join("1").join("p.png").exists());
        assert!(!to.join("gallery").exists());
        assert_eq!(paths.models(), models);
    }

    #[test]
    fn a_models_move_moves_only_the_app_subfolders() {
        let (dir, paths) = home();
        std::fs::write(paths.models().join("mine.txt"), "keep").unwrap();
        let to = dir.path().join("m");
        let plan = plan(&paths, Purpose::Models, &to).unwrap();
        run(&paths, &plan, &mut |_| {}, &CancellationToken::new()).unwrap();
        assert_eq!(paths.models(), to);
        assert!(
            paths
                .ollama_models()
                .join("blobs")
                .join("sha256-a")
                .exists()
        );
        assert!(paths.image_models().join("m.gguf").exists());
        assert!(paths.root().join("models").join("mine.txt").exists());
        assert!(paths.gallery().join("1").join("p.png").exists());
    }

    #[test]
    fn a_failed_copy_changes_nothing() {
        let (dir, paths) = home();
        let before = paths.location();
        let to = dir.path().join("pics");
        let plan = plan(&paths, Purpose::Library, &to).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(run(&paths, &plan, &mut |_| {}, &cancel).is_err());
        assert_eq!(paths.location(), before);
        assert_eq!(Location::load(paths.root()), before);
        assert!(paths.gallery().join("1").join("p.png").exists());
        assert!(!to.join("gallery").exists() && !moving(&to.join("gallery")).exists());
        assert!(!paths.backups().join(NOTE).exists());
    }

    #[test]
    fn a_crash_is_ended_at_the_next_start() {
        // Before the switch: the copies go, the old files stay.
        let (dir, paths) = home();
        let to = dir.path().join("pics");
        let plan = plan(&paths, Purpose::Library, &to).unwrap();
        std::fs::write(
            paths.backups().join(NOTE),
            serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(moving(&to.join("gallery"))).unwrap();
        recover(&paths);
        assert!(!moving(&to.join("gallery")).exists());
        assert!(paths.gallery().join("1").join("p.png").exists());

        // After the switch: the old files go.
        let old = paths.gallery();
        std::fs::create_dir_all(to.join("gallery")).unwrap();
        paths.set_location(plan.location.clone()).unwrap();
        std::fs::write(
            paths.backups().join(NOTE),
            serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        recover(&paths);
        assert!(!old.exists() && to.join("gallery").exists());
        assert!(!paths.backups().join(NOTE).exists());
    }

    #[test]
    fn a_folder_inside_the_old_one_is_refused() {
        let (_dir, paths) = home();
        let inner = paths.gallery().join("x");
        assert!(plan(&paths, Purpose::Library, &inner).is_err());
    }
}
