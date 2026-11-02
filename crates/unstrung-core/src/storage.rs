//! What uses disk space, for Settings > Storage. Only reads. Each part is
//! summed from its own fixed folder: never the app folder as a whole,
//! because the library and the models folder can sit inside it.

use crate::paths::AppPaths;
use crate::repair::folder_size;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Below this, the screen warns that the disk is almost full.
pub const LOW_FREE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Part {
    /// chat_models, picture_models, gallery, chats, set_aside, engines, logs, temp
    pub kind: &'static str,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Disk {
    /// The folders on this disk: app, library, models.
    pub folders: Vec<&'static str>,
    pub free: u64,
    pub low: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub parts: Vec<Part>,
    pub disks: Vec<Disk>,
}

fn file_size(path: &Path) -> u64 {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map_or(0, |m| m.len())
}

/// The entries of `dir` whose names start with `prefix`.
fn named(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect()
}

/// A file or a folder, without following links.
fn size(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => folder_size(path),
        Ok(m) if m.is_file() => m.len(),
        _ => 0,
    }
}

fn sum(paths: impl IntoIterator<Item = PathBuf>) -> u64 {
    paths.into_iter().map(|p| size(&p)).sum()
}

pub fn usage(paths: &AppPaths) -> Usage {
    let backups = paths.backups();
    let library = paths.library();
    let (chat_models, shared) = crate::sidecar::models_folder(paths);
    let db = paths.database();
    let chats = ["", "-wal", "-shm"]
        .iter()
        .map(|s| file_size(Path::new(&format!("{}{s}", db.display()))))
        .sum::<u64>()
        + folder_size(&paths.avatars())
        + sum(named(&backups, "daily-"))
        + sum(named(&backups, "before-"));
    // Data that a restore or a Replace moved aside. Nothing removes it by itself.
    let set_aside = sum(named(&backups, "replaced-")) + sum(named(&library, "gallery-replaced-"));
    let temp = crate::repair::temp_bytes(paths)
        + sum(named(&backups, "import-"))
        + sum(named(&backups, "export-"))
        + sum(named(&library, "gallery-import-"));
    let parts = vec![
        Part {
            kind: "chat_models",
            // A shared folder (development) holds models that are not the app's.
            bytes: if shared { 0 } else { folder_size(&chat_models) },
        },
        Part {
            kind: "picture_models",
            bytes: folder_size(&paths.image_models()),
        },
        Part {
            kind: "gallery",
            bytes: folder_size(&paths.gallery()),
        },
        Part {
            kind: "chats",
            bytes: chats,
        },
        Part {
            kind: "set_aside",
            bytes: set_aside,
        },
        Part {
            kind: "engines",
            bytes: folder_size(&paths.engines()),
        },
        Part {
            kind: "logs",
            bytes: folder_size(&paths.logs()),
        },
        Part {
            kind: "temp",
            bytes: temp,
        },
    ];
    let mut disks: Vec<(PathBuf, Disk)> = Vec::new();
    for (folder, path) in [
        ("app", paths.root().to_path_buf()),
        ("library", library),
        ("models", paths.models()),
    ] {
        let Some(info) = crate::hardware::disk_of(&path) else {
            continue;
        };
        match disks.iter_mut().find(|(mount, _)| *mount == info.mount) {
            Some((_, disk)) => disk.folders.push(folder),
            None => disks.push((
                info.mount,
                Disk {
                    folders: vec![folder],
                    free: info.available,
                    low: info.available < LOW_FREE_BYTES,
                },
            )),
        }
    }
    Usage {
        parts,
        disks: disks.into_iter().map(|(_, d)| d).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_part_counts_its_own_folder_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let write = |p: PathBuf, n: usize| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; n]).unwrap();
        };
        write(paths.database(), 100);
        write(paths.avatars().join("a.png"), 10);
        write(paths.backups().join("daily-1-2.db"), 1000);
        write(paths.backups().join("replaced-import-5").join("app.db"), 7);
        write(
            paths
                .library()
                .join("gallery-replaced-5")
                .join("1")
                .join("p.png"),
            3,
        );
        write(paths.backups().join("import-9").join("app.db"), 20);
        write(paths.gallery().join("1").join("p.png"), 50);
        write(paths.image_models().join("m.gguf"), 400);
        write(paths.logs().join("app.log"), 5);
        let usage = usage(&paths);
        let of = |kind: &str| usage.parts.iter().find(|p| p.kind == kind).unwrap().bytes;
        assert_eq!(of("chats"), 1110);
        assert_eq!(of("set_aside"), 10);
        assert_eq!(of("gallery"), 50);
        assert_eq!(of("picture_models"), 400);
        assert_eq!(of("logs"), 5);
        assert!(of("temp") >= 20);
        assert!(!usage.disks.is_empty() && usage.disks[0].folders.contains(&"app"));
    }
}
