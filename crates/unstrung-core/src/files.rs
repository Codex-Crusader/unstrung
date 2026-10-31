//! File writes that a crash or a full disk cannot leave half done. The
//! database has its own journal, so this is for every other file.

use crate::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Antivirus and search indexers can hold a new file open for a moment.
const RENAME_TRIES: u32 = 5;
const RENAME_PAUSE: Duration = Duration::from_millis(50);

/// The temporary name next to `target`: `name.ext.tmp`. Quick repair deletes
/// these when a crash leaves one (see `repair`).
fn temp_name(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    target.with_file_name(name)
}

/// Writes the whole file or nothing: the bytes go to a temporary file in the
/// same folder, go to the disk, and then replace `target` in one rename.
/// Readers see the old file or the new file, never part of one.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let temp = temp_name(target);
    let result = write_then_rename(&temp, target, bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn write_then_rename(temp: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::File::create(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let mut tries = 1;
    loop {
        match std::fs::rename(temp, target) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && tries < RENAME_TRIES => {
                tries += 1;
                std::thread::sleep(RENAME_PAUSE);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("c1.png");
        write_atomic(&target, b"old").unwrap();
        write_atomic(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_write_keeps_the_old_state_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        // A folder in the way: the rename fails.
        let target = dir.path().join("taken");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, b"data").is_err());
        assert!(target.is_dir());
        assert!(!temp_name(&target).exists());
    }

    #[test]
    fn files_with_the_same_stem_do_not_share_a_temp_file() {
        let target = Path::new("x/c1.png");
        assert_ne!(temp_name(target), temp_name(Path::new("x/c1.json")));
        assert_eq!(temp_name(target).extension().unwrap(), "tmp");
    }
}
