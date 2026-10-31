//! Where the library and the models live (`location.json` in the app folder),
//! and the checks for a folder that the user picks. The app folder itself
//! never moves, so the database never ends up in a synced folder.

use crate::Result;
use crate::files::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const FILE: &str = "location.json";
/// Room to keep free on top of the downloads.
pub const MARGIN_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Marks a library folder, so a reinstall can find it again.
pub(crate) const LIBRARY_MARK: &str = "unstrung-library.json";

/// Saved paths. A missing or damaged file means the defaults, so an install
/// from before this file keeps working.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Gallery pictures. `None` means the app folder.
    #[serde(default)]
    pub library: Option<PathBuf>,
    /// Model files. `None` means `<library>\models`.
    #[serde(default)]
    pub models: Option<PathBuf>,
}

impl Location {
    pub fn load(root: &Path) -> Self {
        match std::fs::read(root.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("{FILE} is damaged, so the default folders are used: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        write_atomic(&root.join(FILE), &serde_json::to_vec_pretty(self)?)
    }

    pub fn library(&self, root: &Path) -> PathBuf {
        self.library.clone().unwrap_or_else(|| root.to_path_buf())
    }

    pub fn models(&self, root: &Path) -> PathBuf {
        self.models
            .clone()
            .unwrap_or_else(|| self.library(root).join("models"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Library,
    Models,
}

/// One finding about a folder. The UI has the words for each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
    /// Program Files, the Windows folder, or the root of the system drive.
    SystemFolder,
    NotAbsolute,
    NotWritable,
    /// OneDrive, Dropbox, Google Drive or iCloud, or cloud placeholder files.
    Synced,
    NetworkDrive,
    RemovableDrive,
    /// Characters outside ASCII. The image engine can fail to open such paths.
    UnusualCharacters,
    NotEnoughSpace,
}

impl Issue {
    /// Blocked issues stop the choice. The others need the user to confirm.
    pub fn blocks(self, purpose: Purpose) -> bool {
        match self {
            Self::SystemFolder | Self::NotAbsolute | Self::NotWritable | Self::NotEnoughSpace => {
                true
            }
            // Files On-Demand can turn a 5 GB model into a placeholder.
            Self::Synced => purpose == Purpose::Models,
            Self::NetworkDrive | Self::RemovableDrive | Self::UnusualCharacters => false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderCheck {
    /// The folder that the app will use: the picked one, or an `Unstrung`
    /// folder inside it when the picked one holds other files.
    pub folder: PathBuf,
    pub issues: Vec<Issue>,
    pub blocked: bool,
    pub free_bytes: u64,
    /// The bytes that the app needs there, with the margin.
    pub need_bytes: u64,
    /// The folder already holds an Unstrung library (for example after a reinstall).
    pub existing_library: bool,
}

/// Checks a folder that the user picked. It writes and deletes one small test
/// file and changes nothing else.
pub fn check(picked: &Path, purpose: Purpose, download_bytes: u64) -> FolderCheck {
    let mut issues = Vec::new();
    let existing_library = picked.join(LIBRARY_MARK).is_file();
    let folder = if existing_library || is_empty_or_missing(picked) {
        picked.to_path_buf()
    } else {
        picked.join("Unstrung")
    };
    if !folder.is_absolute() || folder.to_string_lossy().starts_with(r"\\?\") {
        issues.push(Issue::NotAbsolute);
    }
    if is_system_folder(&folder) {
        issues.push(Issue::SystemFolder);
    }
    if !folder.to_string_lossy().is_ascii() {
        issues.push(Issue::UnusualCharacters);
    }
    if is_synced(&folder) {
        issues.push(Issue::Synced);
    }
    let disk = crate::hardware::disk_of(&folder);
    match &disk {
        None => issues.push(Issue::NetworkDrive),
        Some(d) if d.removable => issues.push(Issue::RemovableDrive),
        Some(_) => {}
    }
    if !writable(&folder) {
        issues.push(Issue::NotWritable);
    }
    let free_bytes = disk.map_or(0, |d| d.available);
    let need_bytes = download_bytes + MARGIN_BYTES;
    if free_bytes < need_bytes {
        issues.push(Issue::NotEnoughSpace);
    }
    let blocked = issues.iter().any(|i| i.blocks(purpose));
    FolderCheck {
        folder,
        issues,
        blocked,
        free_bytes,
        need_bytes,
        existing_library,
    }
}

/// Marks a folder as an Unstrung library after the user picks it.
pub fn mark_library(folder: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    write_atomic(
        &folder.join(LIBRARY_MARK),
        br#"{"app":"Unstrung","kind":"library"}"#,
    )
}

fn is_empty_or_missing(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_none())
}

fn is_system_folder(folder: &Path) -> bool {
    let lower = |p: PathBuf| p.to_string_lossy().to_lowercase();
    let folder = lower(folder.to_path_buf());
    let mut blocked = Vec::new();
    for var in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "SystemRoot",
        "windir",
    ] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            blocked.push(lower(PathBuf::from(v)));
        }
    }
    let inside = blocked
        .iter()
        .any(|b| folder == *b || folder.starts_with(&format!("{}\\", b.trim_end_matches('\\'))));
    // A drive root such as C:\ (the folder has no part after the drive).
    let root = Path::new(&folder)
        .components()
        .all(|c| matches!(c, Component::Prefix(_) | Component::RootDir));
    inside || root
}

/// Known sync folders by path, then cloud file attributes on the folder and
/// its parents, then on a few files inside.
fn is_synced(folder: &Path) -> bool {
    let text = folder.to_string_lossy().to_lowercase();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let v = v.to_string_lossy().to_lowercase();
            if text == v || text.starts_with(&format!("{}\\", v.trim_end_matches('\\'))) {
                return true;
            }
        }
    }
    let named = folder.components().any(|c| {
        let part = c.as_os_str().to_string_lossy().to_lowercase();
        part.starts_with("onedrive")
            || part == "dropbox"
            || part == "google drive"
            || part == "my drive"
            || part == "icloud drive"
            || part == "iclouddrive"
    });
    named || has_cloud_attributes(folder)
}

#[cfg(windows)]
fn has_cloud_attributes(folder: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const PINNED: u32 = 0x8_0000;
    const UNPINNED: u32 = 0x10_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    const CLOUD: u32 = OFFLINE | RECALL_ON_OPEN | PINNED | UNPINNED | RECALL_ON_DATA_ACCESS;
    let cloud =
        |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & CLOUD != 0);
    if folder.ancestors().any(cloud) {
        return true;
    }
    std::fs::read_dir(folder)
        .is_ok_and(|entries| entries.flatten().take(50).any(|e| cloud(&e.path())))
}

#[cfg(not(windows))]
fn has_cloud_attributes(_folder: &Path) -> bool {
    false
}

/// Writes and deletes a test file in the folder, or in the nearest parent
/// that exists when the folder is not made yet.
fn writable(folder: &Path) -> bool {
    let Some(existing) = folder.ancestors().find(|p| p.is_dir()) else {
        return false;
    };
    let probe = existing.join(format!(".unstrung-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_means_the_old_folders() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Location::load(dir.path());
        assert_eq!(loc, Location::default());
        assert_eq!(loc.models(dir.path()), dir.path().join("models"));
        std::fs::write(dir.path().join(FILE), "{ not json").unwrap();
        assert_eq!(Location::load(dir.path()), Location::default());
    }

    #[test]
    fn saved_folders_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let loc = Location {
            library: Some(PathBuf::from(r"D:\Pictures\Unstrung")),
            models: None,
        };
        loc.save(dir.path()).unwrap();
        let back = Location::load(dir.path());
        assert_eq!(back, loc);
        assert_eq!(
            back.models(dir.path()),
            PathBuf::from(r"D:\Pictures\Unstrung\models")
        );
    }

    #[test]
    fn a_folder_with_other_files_gets_an_unstrung_folder() {
        let dir = tempfile::tempdir().unwrap();
        let empty = check(dir.path(), Purpose::Models, 0);
        assert_eq!(empty.folder, dir.path());
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let full = check(dir.path(), Purpose::Models, 0);
        assert_eq!(full.folder, dir.path().join("Unstrung"));
        mark_library(&dir.path().join("Unstrung")).unwrap();
        let again = check(&dir.path().join("Unstrung"), Purpose::Library, 0);
        assert!(again.existing_library);
        // The write test leaves nothing behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn system_folders_and_drive_roots_are_blocked() {
        let windows =
            std::env::var_os("SystemRoot").map_or(PathBuf::from(r"C:\Windows"), PathBuf::from);
        let c = check(&windows.join("Unstrung"), Purpose::Models, 0);
        assert!(c.issues.contains(&Issue::SystemFolder) && c.blocked);
        assert!(is_system_folder(Path::new(r"C:\")));
        assert!(!is_system_folder(Path::new(r"C:\Games\Unstrung")));
    }

    #[test]
    fn sync_folders_block_models_but_only_warn_for_the_library() {
        assert!(is_synced(Path::new(
            r"C:\Users\a\OneDrive - School\Unstrung"
        )));
        assert!(is_synced(Path::new(r"D:\Dropbox\Unstrung")));
        assert!(!is_synced(Path::new(r"D:\Games\Unstrung")));
        assert!(Issue::Synced.blocks(Purpose::Models));
        assert!(!Issue::Synced.blocks(Purpose::Library));
    }

    #[test]
    fn too_little_space_is_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let c = check(dir.path(), Purpose::Models, u64::MAX / 2);
        assert!(c.issues.contains(&Issue::NotEnoughSpace) && c.blocked);
    }
}
