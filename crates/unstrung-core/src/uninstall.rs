//! Deletes the user's data when the uninstaller asks for it ("Also delete my
//! chats, characters, pictures and models"). Only the app knows the folders
//! from `location.json` and which models folder another Ollama shares, so the
//! uninstaller runs the app with `--delete-data` and does not delete folders
//! itself.
//!
//! It deletes the app folder, the library folder when it carries the
//! library mark, and the app's own models. It never deletes a folder that
//! holds a shared models folder, and it never runs while Unstrung runs.

use crate::location;
use crate::paths::AppPaths;
use crate::{sidecar, start};
use std::path::Path;

/// The argument that the uninstaller passes.
pub const DELETE_ARG: &str = "--delete-data";

/// Deletes the data. Returns what it could not delete, empty when all is gone.
pub fn delete_data(paths: &AppPaths) -> Vec<String> {
    let root = paths.root().to_path_buf();
    if !root.exists() {
        return Vec::new();
    }
    if !is_app_folder(&root) {
        return vec![format!("{} is not an Unstrung folder", root.display())];
    }
    let lock = match start::lock(paths) {
        Ok(Some(lock)) => lock,
        Ok(None) => return vec!["Unstrung is running".to_owned()],
        Err(e) => return vec![format!("{}: {e}", paths.lock_file().display())],
    };
    // An engine from an earlier run holds model files open.
    sidecar::stop_leftover(paths);

    let (models, shared) = sidecar::models_folder(paths);
    let keep = shared.then_some(models);
    let mut left = Vec::new();
    let remove = |dir: &Path, left: &mut Vec<String>| {
        if keep.as_ref().is_some_and(|k| k.starts_with(dir)) {
            left.push(format!("{} holds a shared models folder", dir.display()));
        } else if dir.is_dir()
            && let Err(e) = std::fs::remove_dir_all(dir)
        {
            left.push(format!("{}: {e}", dir.display()));
        }
    };

    // The app's own models, then the folders from `location.json`.
    remove(&paths.ollama_models(), &mut left);
    remove(&paths.image_models(), &mut left);
    let custom_models = paths.models();
    if !custom_models.starts_with(&root) {
        // A folder that only held the engine's models is empty now.
        let _ = std::fs::remove_dir(&custom_models);
    }
    let library = paths.library();
    if !library.starts_with(&root) && library.is_dir() {
        if library.join(location::LIBRARY_MARK).is_file() {
            remove(&library, &mut left);
        } else {
            left.push(format!(
                "{} has no library mark, so it stays",
                library.display()
            ));
        }
    }

    // The app folder, except the lock, which this process holds.
    let lock_file = paths.lock_file();
    match std::fs::read_dir(&root) {
        Ok(entries) => {
            for path in entries.flatten().map(|e| e.path()) {
                if path == lock_file {
                    continue;
                }
                if path.is_dir() {
                    remove(&path, &mut left);
                } else if let Err(e) = std::fs::remove_file(&path) {
                    left.push(format!("{}: {e}", path.display()));
                }
            }
        }
        Err(e) => left.push(format!("{}: {e}", root.display())),
    }
    drop(lock);
    let _ = std::fs::remove_file(&lock_file);
    if left.is_empty()
        && let Err(e) = std::fs::remove_dir(&root)
    {
        left.push(format!("{}: {e}", root.display()));
    }
    left
}

/// A folder that Unstrung made: it has the database, the location file or
/// the engines folder. A wrong `UNSTRUNG_HOME` cannot point the delete at
/// another folder.
fn is_app_folder(root: &Path) -> bool {
    let has = |name: &str| root.join(name).exists();
    has("app.db") || has(location::FILE) || has("engines")
}

/// The app folder of a test, with the files that a real one has.
#[cfg(test)]
fn fake_app_folder(root: &Path) -> Vec<std::path::PathBuf> {
    let files = [
        "app.db",
        "app.db-wal",
        "avatars/a.png",
        "logs/app.log",
        "engines/ollama/ollama.exe",
        "models/ollama/blobs/sha256-1",
    ];
    files
        .iter()
        .map(|f| {
            let path = root.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
            path
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::Location;

    #[test]
    fn the_app_folder_and_a_marked_library_go() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Unstrung");
        fake_app_folder(&root);
        let library = dir.path().join("Library");
        location::mark_library(&library).unwrap();
        std::fs::write(library.join("picture.png"), b"x").unwrap();
        let paths = AppPaths::at(root.clone());
        paths
            .set_location(Location {
                library: Some(library.clone()),
                ..Location::default()
            })
            .unwrap();
        let paths = AppPaths::at(root.clone());
        assert_eq!(delete_data(&paths), Vec::<String>::new());
        assert!(!root.exists());
        assert!(!library.exists());
    }

    #[test]
    fn picture_models_in_a_models_folder_outside_go() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Unstrung");
        fake_app_folder(&root);
        let models = dir.path().join("Models");
        AppPaths::at(root.clone())
            .set_location(Location {
                models: Some(models.clone()),
                ..Location::default()
            })
            .unwrap();
        let paths = AppPaths::at(root.clone());
        std::fs::create_dir_all(paths.image_models()).unwrap();
        std::fs::write(paths.image_models().join("z.gguf"), b"x").unwrap();
        assert_eq!(delete_data(&paths), Vec::<String>::new());
        assert!(!models.exists());
    }

    #[test]
    fn a_library_without_the_mark_stays() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Unstrung");
        fake_app_folder(&root);
        let library = dir.path().join("Documents");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("thesis.docx"), b"x").unwrap();
        AppPaths::at(root.clone())
            .set_location(Location {
                library: Some(library.clone()),
                ..Location::default()
            })
            .unwrap();
        let left = delete_data(&AppPaths::at(root.clone()));
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(library.join("thesis.docx").is_file());
    }

    #[test]
    fn a_folder_that_is_not_unstrung_stays() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let left = delete_data(&AppPaths::at(dir.path().to_path_buf()));
        assert_eq!(left.len(), 1);
        assert!(dir.path().join("notes.txt").is_file());
    }

    #[test]
    fn nothing_happens_while_the_app_runs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Unstrung");
        let files = fake_app_folder(&root);
        let paths = AppPaths::at(root);
        let _running = start::lock(&paths).unwrap().unwrap();
        assert_eq!(delete_data(&paths), ["Unstrung is running"]);
        assert!(files.iter().all(|f| f.is_file()));
    }
}
