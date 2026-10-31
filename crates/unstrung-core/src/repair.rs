//! Quick repair for safe start. `check` only looks. `fix` checks again and
//! fixes what it can, so it never acts on an old list. Neither one touches
//! chats, characters, personas or pictures.

use crate::avatar::STAGING;
use crate::db::Db;
use crate::engine_setup;
use crate::hardware::Hardware;
use crate::paths::AppPaths;
use crate::sidecar;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;

/// Download parts and engine partial files older than this are left over.
const OLD_PART: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// A chat engine that an earlier run did not stop.
    Engine,
    /// The chat engine's files against the hash list of the release.
    EngineFiles,
    /// The database with the chats.
    Data,
    /// The library and models folders from `location.json`.
    Location,
    /// Each installed model has all its files.
    Models,
    /// The hardware check, run again.
    Hardware,
    /// Upload and half-written files, old download parts and old logs.
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
    run(paths, false).await
}

/// Checks again and fixes what it can.
pub async fn fix(paths: &AppPaths) -> Vec<Finding> {
    run(paths, true).await
}

async fn run(paths: &AppPaths, fix: bool) -> Vec<Finding> {
    // The file checks read gigabytes, so they run off the async threads,
    // while the database check runs here.
    let blocking = {
        let paths = paths.clone();
        tokio::task::spawn_blocking(move || {
            [
                engine_files(&paths, fix),
                location(&paths),
                models(&paths, fix),
                hardware(&paths),
                temp_files(&paths, fix),
            ]
        })
    };
    let engine = engine(paths, fix);
    let data = data(paths).await;
    match blocking.await {
        Ok([engine_files, location, models, hardware, temp]) => {
            vec![engine, engine_files, data, location, models, hardware, temp]
        }
        Err(e) => {
            let mut f = Finding::new(Area::TempFiles, State::NeedsAction);
            f.detail = format!("the file checks stopped: {e}");
            vec![engine, data, f]
        }
    }
}

/// An engine with changed or missing files. The fix removes the install
/// mark, so the next start offers "Install", which replaces the folder.
fn engine_files(paths: &AppPaths, fix: bool) -> Finding {
    if engine_setup::installed_version(paths).is_none() {
        return Finding::new(Area::EngineFiles, State::Ok);
    }
    match engine_setup::verify(paths, &CancellationToken::new()) {
        Ok(bad) if bad.is_empty() => Finding::new(Area::EngineFiles, State::Ok),
        Ok(bad) => {
            let mut f = Finding::new(Area::EngineFiles, State::CanFix);
            f.detail = list(&bad);
            if fix {
                f.state = match engine_setup::forget_install(paths) {
                    Ok(()) => State::Fixed,
                    Err(e) => {
                        f.detail = format!("{e}\n{}", f.detail);
                        State::NeedsAction
                    }
                };
            }
            f
        }
        Err(e) => {
            let mut f = Finding::new(Area::EngineFiles, State::NeedsAction);
            f.detail = e.to_string();
            f
        }
    }
}

/// Only reports: picking another folder is the user's choice.
fn location(paths: &AppPaths) -> Finding {
    let mut problems = Vec::new();
    for folder in [paths.library(), paths.models()] {
        if !folder.is_dir() {
            problems.push(format!("missing: {}", folder.display()));
            continue;
        }
        let probe = folder.join(".unstrung-write-test");
        match std::fs::write(&probe, b"") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
            }
            Err(e) => problems.push(format!("cannot write to {}: {e}", folder.display())),
        }
    }
    let mut f = Finding::new(Area::Location, State::Ok);
    if !problems.is_empty() {
        f.state = State::NeedsAction;
        f.detail = problems.join("\n");
    }
    f
}

/// A model whose manifest names a file that is gone. The fix removes only
/// that manifest, so the Models screen offers the model for download again.
/// A folder that Unstrung shares with another Ollama is only reported.
fn models(paths: &AppPaths, fix: bool) -> Finding {
    let (folder, shared) = sidecar::models_folder(paths);
    let broken = broken_models(&folder);
    if broken.is_empty() {
        return Finding::new(Area::Models, State::Ok);
    }
    let names: Vec<String> = broken
        .iter()
        .map(|m| {
            m.strip_prefix(folder.join("manifests"))
                .unwrap_or(m)
                .display()
                .to_string()
        })
        .collect();
    let mut f = Finding::new(
        Area::Models,
        if shared {
            State::NeedsAction
        } else {
            State::CanFix
        },
    );
    f.detail = list(&names);
    if fix && !shared {
        let failed: Vec<String> = broken
            .iter()
            .filter_map(|m| {
                std::fs::remove_file(m)
                    .err()
                    .map(|e| format!("{}: {e}", m.display()))
            })
            .collect();
        f.state = if failed.is_empty() {
            State::Fixed
        } else {
            State::NeedsAction
        };
        if !failed.is_empty() {
            f.detail = failed.join("\n");
        }
    }
    f
}

/// Manifests (`manifests/<host>/<namespace>/<model>/<tag>`) that name a
/// blob which is not in `blobs`. A manifest that does not parse is broken too.
fn broken_models(folder: &Path) -> Vec<PathBuf> {
    let mut manifests = Vec::new();
    collect_files(&folder.join("manifests"), 3, &mut manifests);
    manifests
        .into_iter()
        .filter(|m| {
            let Ok(text) = std::fs::read(m) else {
                return true;
            };
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(&text) else {
                return true;
            };
            let config = json.get("config").into_iter();
            let layers = json
                .get("layers")
                .and_then(|l| l.as_array())
                .into_iter()
                .flatten();
            config.chain(layers).any(|layer| {
                let digest = layer.get("digest").and_then(|d| d.as_str()).unwrap_or("");
                let Some(hex) = digest.strip_prefix("sha256:") else {
                    return true;
                };
                !folder.join("blobs").join(format!("sha256-{hex}")).is_file()
            })
        })
        .collect()
}

/// The files in the folders `depth` levels below `dir` (0: the files in `dir`).
fn collect_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if depth == 0 {
            if path.is_file() {
                out.push(path);
            }
        } else if path.is_dir() {
            collect_files(&path, depth - 1, out);
        }
    }
}

/// Runs the hardware check again. It only reports: the Models screen shows
/// the tier, and a new tier takes effect at the next start.
fn hardware(paths: &AppPaths) -> Finding {
    let hw = Hardware::check(&sidecar::models_folder(paths).0);
    let mut f = Finding::new(Area::Hardware, State::Ok);
    f.detail = format!(
        "{}, {} memory, tier {:?}",
        hw.gpu.as_ref().map_or_else(
            || "no graphics card for AI".to_owned(),
            |g| format!("{}, {}", g.name, gb(g.vram_bytes))
        ),
        gb(hw.ram_bytes),
        hw.tier
    );
    f
}

/// A memory size, rounded to a tenth: "8.0 GB" (a card sold as 8 GB has 8 GiB).
fn gb(bytes: u64) -> String {
    let tenths = (bytes.saturating_mul(10) + (1 << 29)) >> 30;
    format!("{}.{} GB", tenths / 10, tenths % 10)
}

/// At most 10 lines, for Details.
fn list(items: &[String]) -> String {
    let mut text = items
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if items.len() > 10 {
        text.push_str(&format!("\n... and {} more", items.len() - 10));
    }
    text
}

/// Old download parts: Unstrung's own `.part` files and the engine's
/// partial model files, older than `OLD_PART`. Not in a shared folder.
fn old_parts(paths: &AppPaths) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut look = |dir: PathBuf, part: &dyn Fn(&str) -> bool| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > OLD_PART);
            if old && part(&name) && path.is_file() {
                found.push(path);
            }
        }
    };
    look(paths.engines().join(".download"), &|n| n.ends_with(".part"));
    let (models, shared) = sidecar::models_folder(paths);
    if !shared {
        look(models.join("blobs"), &|n| n.contains("-partial"));
    }
    found
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
    // The rotated logs (`app.log.1`): the current logs stay for the report.
    if let Ok(entries) = std::fs::read_dir(paths.logs()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.to_string_lossy().ends_with(".log.1") && path.is_file() {
                files.push(path);
            }
        }
    }
    files.extend(old_parts(paths));
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

    #[test]
    fn memory_sizes_round_to_a_tenth() {
        assert_eq!(gb(8_585_740_288), "8.0 GB");
        assert_eq!(gb(16_441_221_120), "15.3 GB");
        assert_eq!(gb(0), "0.0 GB");
    }

    #[test]
    fn a_manifest_with_a_missing_file_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let lib = folder.join("manifests/registry.ollama.ai/library");
        std::fs::create_dir_all(folder.join("blobs")).unwrap();
        std::fs::write(folder.join("blobs/sha256-aa"), b"x").unwrap();
        std::fs::write(folder.join("blobs/sha256-bb"), b"x").unwrap();
        let manifest = |model: &str, digests: &[&str]| {
            std::fs::create_dir_all(lib.join(model)).unwrap();
            let layers: Vec<_> = digests
                .iter()
                .map(|d| serde_json::json!({ "digest": format!("sha256:{d}") }))
                .collect();
            let json = serde_json::json!({ "config": layers[0], "layers": layers[1..] });
            std::fs::write(lib.join(model).join("1"), json.to_string()).unwrap();
        };
        manifest("good", &["aa", "bb"]);
        manifest("gone", &["aa", "cc"]);
        std::fs::create_dir_all(lib.join("junk")).unwrap();
        std::fs::write(lib.join("junk").join("1"), b"not json").unwrap();
        let mut broken = broken_models(folder);
        broken.sort();
        assert_eq!(
            broken,
            [lib.join("gone").join("1"), lib.join("junk").join("1")]
        );
    }

    #[tokio::test]
    async fn old_logs_and_old_parts_go_but_new_ones_stay() {
        let (_dir, paths) = paths();
        std::fs::write(paths.logs().join("app.log"), [0u8; 10]).unwrap();
        std::fs::write(paths.logs().join("app.log.1"), [0u8; 30]).unwrap();
        let downloads = paths.engines().join(".download");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("new.zip.part"), [0u8; 5]).unwrap();
        let old = downloads.join("old.zip.part");
        std::fs::write(&old, [0u8; 7]).unwrap();
        let ten_days = Duration::from_secs(10 * 24 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - ten_days)
            .unwrap();
        let found = temp_files(&paths, false);
        assert_eq!((found.state, found.bytes), (State::CanFix, 37));
        assert_eq!(temp_files(&paths, true).state, State::Fixed);
        assert!(!paths.logs().join("app.log.1").exists() && !old.exists());
        assert!(paths.logs().join("app.log").exists() && downloads.join("new.zip.part").exists());
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
