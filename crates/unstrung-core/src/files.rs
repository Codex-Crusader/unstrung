//! File writes that a crash or a full disk cannot leave half done. The
//! database has its own journal, so this is for every other file.

use crate::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Antivirus scans, search indexers and an engine process that is still
/// closing can hold a file for a few seconds on a real PC.
const RENAME_WAIT: Duration = Duration::from_secs(3);

/// `std::fs::rename` that waits while another program holds the file, or a
/// file inside the folder. Windows then refuses with access denied (5), a
/// sharing violation (32) or a lock violation (33).
pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + RENAME_WAIT;
    let mut pause = Duration::from_millis(50);
    loop {
        match std::fs::rename(from, to) {
            Err(e) if held(&e) && Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(500));
            }
            done => return done,
        }
    }
}

fn held(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33))
}

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
    Ok(rename(temp, target)?)
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

    /// A scanner holds a file in the folder for a moment: the folder still
    /// moves (the engine swap), and so does a held file (a finished download).
    #[cfg(windows)]
    #[test]
    fn a_rename_waits_while_another_program_holds_the_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("ollama.new");
        std::fs::create_dir(&folder).unwrap();
        let file = folder.join("ollama.exe");
        std::fs::write(&file, b"exe").unwrap();
        for (from, to) in [
            (folder.clone(), dir.path().join("ollama")),
            (
                dir.path().join("ollama").join("ollama.exe"),
                dir.path().join("moved.exe"),
            ),
        ] {
            let inside = if from.is_dir() {
                from.join("ollama.exe")
            } else {
                from.clone()
            };
            // No sharing at all, as a scanner or a running program opens it.
            let held = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&inside)
                .unwrap();
            assert!(
                std::fs::rename(&from, &to).is_err(),
                "{from:?} must be held"
            );
            let release = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                drop(held);
            });
            rename(&from, &to).unwrap();
            release.join().unwrap();
            assert!(to.exists() && !from.exists());
        }
    }

    #[test]
    fn files_with_the_same_stem_do_not_share_a_temp_file() {
        let target = Path::new("x/c1.png");
        assert_ne!(temp_name(target), temp_name(Path::new("x/c1.json")));
        assert_eq!(temp_name(target).extension().unwrap(), "tmp");
    }
}
