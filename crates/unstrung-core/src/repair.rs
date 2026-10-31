//! Quick repair for safe start. `check` only looks. `fix` checks again and
//! fixes what it can, so it never acts on an old list. Neither one touches
//! chats, characters, personas or pictures.

use crate::avatar::STAGING;
use crate::db::Db;
use crate::paths::AppPaths;
use crate::sidecar;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// A chat engine that an earlier run did not stop.
    Engine,
    /// The database with the chats.
    Data,
    /// Upload and half-written files.
    TempFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    /// `fix` can fix it.
    CanFix,
    Fixed,
    /// The app cannot fix it. The user must act, for example ask for help.
    NeedsAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub area: Area,
    pub state: State,
    /// Bytes of temporary files, found or freed.
    pub bytes: u64,
    /// Technical detail for "Details". Empty when there is none.
    pub detail: String,
}

impl Finding {
    fn new(area: Area, state: State) -> Self {
        Self {
            area,
            state,
            bytes: 0,
            detail: String::new(),
        }
    }
}

/// Looks for problems and changes nothing.
pub async fn check(paths: &AppPaths) -> Vec<Finding> {
    vec![
        engine(paths, false),
        data(paths).await,
        temp_files(paths, false),
    ]
}

/// Checks again and fixes what it can.
pub async fn fix(paths: &AppPaths) -> Vec<Finding> {
    vec![
        engine(paths, true),
        data(paths).await,
        temp_files(paths, true),
    ]
}

fn engine(paths: &AppPaths, fix: bool) -> Finding {
    match sidecar::leftover(paths) {
        None => Finding::new(Area::Engine, State::Ok),
        Some(pid) if fix => {
            sidecar::stop_leftover(paths);
            let mut f = Finding::new(Area::Engine, State::Fixed);
            f.detail = format!("stopped process {pid}");
            f
        }
        Some(pid) => {
            let mut f = Finding::new(Area::Engine, State::CanFix);
            f.detail = format!("process {pid}");
            f
        }
    }
}

/// Only reports. Restoring a snapshot is a separate step that the user picks.
async fn data(paths: &AppPaths) -> Finding {
    match Db::integrity_check_file(&paths.database()).await {
        Ok(None) => Finding::new(Area::Data, State::Ok),
        Ok(Some(problems)) if problems.is_empty() => Finding::new(Area::Data, State::Ok),
        Ok(Some(problems)) => {
            let mut f = Finding::new(Area::Data, State::NeedsAction);
            f.detail = problems.join("\n");
            f
        }
        Err(e) => {
            let mut f = Finding::new(Area::Data, State::NeedsAction);
            f.detail = e.to_string();
            f
        }
    }
}

fn temp_files(paths: &AppPaths, fix: bool) -> Finding {
    let mut files = Vec::new();
    let staging = paths.root().join(STAGING);
    let staging_bytes = folder_size(&staging);
    // Half-written files from `files::write_atomic`, which writes `*.tmp` first.
    for dir in [
        paths.root(),
        paths.avatars().as_path(),
        paths.logs().as_path(),
    ] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "tmp") && path.is_file() {
                    files.push(path);
                }
            }
        }
    }
    let bytes = staging_bytes + files.iter().map(|p| file_size(p)).sum::<u64>();
    if bytes == 0 && files.is_empty() && !staging.exists() {
        return Finding::new(Area::TempFiles, State::Ok);
    }
    let mut f = Finding::new(Area::TempFiles, State::CanFix);
    f.bytes = bytes;
    if !fix {
        return f;
    }
    let mut failed = Vec::new();
    if staging.exists()
        && let Err(e) = std::fs::remove_dir_all(&staging)
    {
        failed.push(format!("{}: {e}", staging.display()));
    }
    for path in &files {
        if let Err(e) = std::fs::remove_file(path) {
            failed.push(format!("{}: {e}", path.display()));
        }
    }
    f.state = if failed.is_empty() {
        State::Fixed
    } else {
        State::NeedsAction
    };
    f.detail = failed.join("\n");
    f
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                folder_size(&path)
            } else {
                file_size(&path)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        (dir, paths)
    }

    #[tokio::test]
    async fn a_clean_folder_passes() {
        let (_dir, paths) = paths();
        Db::open(&paths.database()).await.unwrap().close().await;
        let found = check(&paths).await;
        assert!(found.iter().all(|f| f.state == State::Ok), "{found:?}");
    }

    #[tokio::test]
    async fn temp_files_are_found_then_deleted() {
        let (_dir, paths) = paths();
        std::fs::create_dir_all(paths.root().join(STAGING)).unwrap();
        std::fs::write(paths.root().join(STAGING).join("up.png"), [0u8; 100]).unwrap();
        std::fs::write(paths.avatars().join("c1.tmp"), [0u8; 20]).unwrap();
        std::fs::write(paths.avatars().join("c1.png"), [0u8; 50]).unwrap();
        let found = temp_files(&paths, false);
        assert_eq!((found.state, found.bytes), (State::CanFix, 120));
        assert!(paths.avatars().join("c1.tmp").exists());
        let fixed = temp_files(&paths, true);
        assert_eq!(fixed.state, State::Fixed);
        assert!(!paths.root().join(STAGING).exists());
        assert!(!paths.avatars().join("c1.tmp").exists());
        // Real pictures stay.
        assert!(paths.avatars().join("c1.png").exists());
        assert_eq!(temp_files(&paths, false).state, State::Ok);
    }

    #[tokio::test]
    async fn damaged_data_needs_the_user_and_stays_unchanged() {
        let (_dir, paths) = paths();
        let junk = b"this is not a database".repeat(100);
        std::fs::write(paths.database(), &junk).unwrap();
        let found = data(&paths).await;
        assert_eq!(found.state, State::NeedsAction);
        assert!(!found.detail.is_empty());
        assert_eq!(std::fs::read(paths.database()).unwrap(), junk);
    }

    #[test]
    fn a_process_that_is_not_the_engine_never_matches() {
        let (_dir, paths) = paths();
        // This test process: alive, with the right start time, but not ollama.exe.
        let pid = std::process::id();
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
            true,
        );
        let start_time = system
            .process(sysinfo::Pid::from_u32(pid))
            .unwrap()
            .start_time();
        let record = format!(r#"{{"pid":{pid},"start_time":{start_time}}}"#);
        std::fs::write(paths.engine_pids(), record).unwrap();
        assert_eq!(sidecar::leftover(&paths), None);
        assert_eq!(engine(&paths, false).state, State::Ok);
    }
}
