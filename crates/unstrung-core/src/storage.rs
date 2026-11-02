//! What uses disk space, for Settings > Storage. Only reads. Each part is
//! summed from its own fixed folder: never the app folder as a whole,
//! because the library and the models folder can sit inside it.

use crate::db::{Db, now_ms};
use crate::error::Result;
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

/// One line of "Free up space". The UI sends back only the `id`.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// temp, deleted_chats or set_aside
    pub id: &'static str,
    pub bytes: u64,
    /// For "View": what goes.
    pub items: Vec<String>,
    /// Safe to remove, so it starts checked.
    pub checked: bool,
}

/// A folder that a restore (`replaced-<n>`) or a Replace
/// (`replaced-import-<n>`, `gallery-replaced-<n>`) set aside. Exact names
/// only, so nothing that merely looks alike is removed.
fn set_aside_name(name: &str) -> bool {
    ["replaced-import-", "replaced-", "gallery-replaced-"]
        .iter()
        .filter_map(|p| name.strip_prefix(p))
        .any(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

fn set_aside(paths: &AppPaths) -> Vec<PathBuf> {
    [paths.backups(), paths.library()]
        .iter()
        .flat_map(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| set_aside_name(&e.file_name().to_string_lossy()))
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
        })
        .collect()
}

/// What "Free up space" offers, largest first. Models are not here: the
/// Models screen removes them and knows which ones are in use.
pub async fn candidates(db: &Db, paths: &AppPaths) -> Result<Vec<Candidate>> {
    let mut list = Vec::new();
    let temp = crate::repair::temp_bytes(paths);
    if temp > 0 {
        list.push(Candidate {
            id: "temp",
            bytes: temp,
            items: Vec::new(),
            checked: true,
        });
    }
    let deleted = db.deleted_chats().await?;
    if !deleted.is_empty() {
        list.push(Candidate {
            id: "deleted_chats",
            bytes: deleted
                .iter()
                .map(|c| folder_size(&paths.gallery().join(c.chat_id.to_string())))
                .sum(),
            items: deleted
                .iter()
                .map(|c| format!("{}: {}", c.character_name, c.preview))
                .collect(),
            checked: false,
        });
    }
    let aside = set_aside(paths);
    if !aside.is_empty() {
        list.push(Candidate {
            id: "set_aside",
            bytes: aside.iter().map(|p| folder_size(p)).sum(),
            items: aside.iter().map(|p| p.display().to_string()).collect(),
            checked: false,
        });
    }
    list.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    Ok(list)
}

/// Removes the candidates named in `ids` that are still offered, and
/// returns the bytes they held. An unknown or gone id does nothing.
pub async fn free_up(db: &Db, paths: &AppPaths, ids: &[String]) -> Result<u64> {
    let mut freed = 0;
    for c in candidates(db, paths).await? {
        if !ids.iter().any(|id| id == c.id) {
            continue;
        }
        match c.id {
            "temp" => {
                let failed = crate::repair::remove_temp_files(paths);
                if !failed.is_empty() {
                    log::warn!("some temporary files stayed: {failed}");
                }
            }
            // All of Recently deleted: the cut-off lies 7 days ahead.
            "deleted_chats" => {
                crate::library::purge_deleted_chats(
                    db,
                    paths,
                    now_ms() + crate::library::KEEP_DELETED_MS,
                )
                .await?;
            }
            _ => {
                for dir in set_aside(paths) {
                    if let Err(e) = std::fs::remove_dir_all(&dir) {
                        log::warn!("could not remove {}: {e}", dir.display());
                    }
                }
            }
        }
        freed += c.bytes;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn free_up_removes_only_what_it_offers() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let db = Db::open(&paths.database()).await.unwrap();
        for name in ["replaced-123", "replaced-import-456"] {
            std::fs::create_dir_all(paths.backups().join(name)).unwrap();
            std::fs::write(paths.backups().join(name).join("app.db"), [0u8; 10]).unwrap();
        }
        // Names that only look alike stay.
        for name in ["replaced-12a", "replaced-", "daily-1-2.db"] {
            std::fs::create_dir_all(paths.backups().join(name)).unwrap();
        }
        let offered = candidates(&db, &paths).await.unwrap();
        let aside = offered.iter().find(|c| c.id == "set_aside").unwrap();
        assert_eq!(
            (aside.bytes, aside.items.len(), aside.checked),
            (20, 2, false)
        );

        // An id that is not offered does nothing.
        assert_eq!(free_up(&db, &paths, &["models".into()]).await.unwrap(), 0);
        assert!(paths.backups().join("replaced-123").exists());

        assert_eq!(
            free_up(&db, &paths, &["set_aside".into()]).await.unwrap(),
            20
        );
        assert!(!paths.backups().join("replaced-123").exists());
        assert!(!paths.backups().join("replaced-import-456").exists());
        for name in ["replaced-12a", "replaced-", "daily-1-2.db"] {
            assert!(paths.backups().join(name).exists(), "{name}");
        }
        assert!(
            candidates(&db, &paths)
                .await
                .unwrap()
                .iter()
                .all(|c| c.id != "set_aside")
        );
    }

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
