//! Safe start. The app counts each start in a small file and the window
//! clears the count when it has loaded. After 2 starts in a row that did not
//! load, the next start opens the safe start screen: no engines, no chats,
//! only repair tools.

use crate::Result;
use crate::paths::AppPaths;
use serde::Serialize;

/// Starts in a row that did not load before the next start is a safe start.
pub const FAILED_STARTS: u32 = 2;

/// Why the app opened in safe start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeReason {
    /// The last starts did not load.
    FailedStarts,
    /// The user asked: `--safe-start`, or Shift held while the app opens.
    Asked,
    /// The chat data did not open.
    DataDidNotOpen,
}

/// Locks the app folder for this process, so two Unstrung processes never
/// open the same database. Keep the file open until the process ends. Windows
/// frees the lock when the process ends, also after a crash, so a lock is
/// never left behind. `Ok(None)` means another process holds the folder.
pub fn lock(paths: &AppPaths) -> Result<Option<std::fs::File>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.lock_file())?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// The starts in a row that did not load. A missing or damaged file counts as 0.
pub fn attempts(paths: &AppPaths) -> u32 {
    std::fs::read_to_string(paths.start_attempts())
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Counts one more start. Call it before anything that can crash.
pub fn record_attempt(paths: &AppPaths) -> Result<()> {
    let next = attempts(paths).saturating_add(1);
    crate::starters::write_atomic(&paths.start_attempts(), next.to_string().as_bytes())
}

/// Clears the count after a start that loaded.
pub fn clear(paths: &AppPaths) -> Result<()> {
    match std::fs::remove_file(paths.start_attempts()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Whether this start is a safe start. `attempts` is the count before this start.
pub fn decide(attempts: u32, asked: bool) -> Option<SafeReason> {
    if asked {
        Some(SafeReason::Asked)
    } else if attempts >= FAILED_STARTS {
        Some(SafeReason::FailedStarts)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_failed_starts_open_safe_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        assert_eq!(attempts(&paths), 0);
        assert_eq!(decide(attempts(&paths), false), None);
        record_attempt(&paths).unwrap();
        assert_eq!(decide(attempts(&paths), false), None);
        record_attempt(&paths).unwrap();
        assert_eq!(attempts(&paths), 2);
        assert_eq!(
            decide(attempts(&paths), false),
            Some(SafeReason::FailedStarts)
        );
        clear(&paths).unwrap();
        clear(&paths).unwrap();
        assert_eq!(attempts(&paths), 0);
    }

    #[test]
    fn only_one_holder_of_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        let first = lock(&paths).unwrap();
        assert!(first.is_some());
        assert!(lock(&paths).unwrap().is_none());
        drop(first);
        assert!(lock(&paths).unwrap().is_some());
    }

    #[test]
    fn a_damaged_count_is_zero_and_asking_always_wins() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        std::fs::write(paths.start_attempts(), "not a number").unwrap();
        assert_eq!(attempts(&paths), 0);
        record_attempt(&paths).unwrap();
        assert_eq!(attempts(&paths), 1);
        assert_eq!(decide(0, true), Some(SafeReason::Asked));
        assert_eq!(decide(5, true), Some(SafeReason::Asked));
    }
}
