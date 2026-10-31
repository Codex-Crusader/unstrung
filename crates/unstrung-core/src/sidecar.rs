//! Starts and stops the private Ollama process.
//!
//! The engine gets its own port on 127.0.0.1 and its own models folder,
//! so it never touches an Ollama that the user runs outside Unstrung.

use crate::error::{Error, Result};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use sysinfo::{Pid, ProcessesToUpdate, System};

const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// Where the engine binary is. `engines/ollama` first. Until the engine download
/// exists (Phase 4), the Ollama that is installed for this Windows user is a fallback.
pub fn find_ollama(paths: &AppPaths) -> Option<PathBuf> {
    let own = paths.engines().join("ollama").join("ollama.exe");
    if own.is_file() {
        return Some(own);
    }
    let installed = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("Programs")
        .join("Ollama")
        .join("ollama.exe");
    installed.is_file().then_some(installed)
}

/// The models folder, and whether another Ollama may also use it.
pub fn models_folder(paths: &AppPaths) -> (PathBuf, bool) {
    if let Some(dev) = std::env::var_os(crate::paths::DEV_MODELS_ENV).filter(|v| !v.is_empty()) {
        return (PathBuf::from(dev), true);
    }
    // Debug builds read the models of the developer's own Ollama, so a test
    // does not download them again. Release builds use the app's own folder.
    if cfg!(debug_assertions)
        && let Some(profile) = std::env::var_os("USERPROFILE")
    {
        let shared = PathBuf::from(profile).join(".ollama").join("models");
        if shared.is_dir() {
            return (shared, true);
        }
    }
    (paths.ollama_models(), false)
}

pub struct Sidecar {
    child: Child,
    port: u16,
    // Closing the job handle kills the engine, also when this app crashes.
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

#[derive(Serialize, Deserialize)]
struct PidRecord {
    pid: u32,
    start_time: u64,
}

impl Sidecar {
    pub fn start(paths: &AppPaths, exe: &Path) -> Result<Self> {
        let port = free_port()?;
        let (models, shared) = models_folder(paths);
        std::fs::create_dir_all(&models)?;
        let log = open_log(&paths.logs().join("ollama.log"))?;

        let mut cmd = Command::new(exe);
        cmd.arg("serve");
        // Settings from the user's own Ollama must not leak in (for example a wide OLLAMA_ORIGINS).
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("OLLAMA_")
            {
                cmd.env_remove(key);
            }
        }
        cmd.env("OLLAMA_HOST", format!("127.0.0.1:{port}"))
            .env("OLLAMA_MODELS", &models)
            // Chats stay on this PC: no cloud models, no web search, no
            // recommendation calls. Downloads still work.
            .env("OLLAMA_NO_CLOUD", "1")
            // Flash attention with an 8-bit context cache needs about half
            // the memory for the chat history, so more of it fits on the GPU.
            .env("OLLAMA_FLASH_ATTENTION", "1")
            .env("OLLAMA_KV_CACHE_TYPE", "q8_0")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        if shared {
            // Do not delete files that the other Ollama is still downloading.
            cmd.env("OLLAMA_NOPRUNE", "1");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = cmd.spawn()?;
        log::info!(
            "chat engine started: pid {}, port {port}, models {}",
            child.id(),
            models.display()
        );
        let sidecar = Self {
            #[cfg(windows)]
            _job: kill_on_close_job(&child),
            child,
            port,
        };
        save_pid(paths, sidecar.child.id());
        Ok(sidecar)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The exit code text if the process ended.
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => None,
            Err(e) => Some(e.to_string()),
        }
    }

    pub fn stop(mut self, paths: &AppPaths) {
        self.kill();
        let _ = std::fs::remove_file(paths.engine_pids());
    }

    fn kill(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            kill_tree(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
fn kill_on_close_job(child: &Child) -> Option<win32job::Job> {
    use std::os::windows::io::AsRawHandle;
    let mut info = win32job::ExtendedLimitInfo::new();
    info.limit_kill_on_job_close();
    let job = win32job::Job::create_with_limit_info(&info)
        .map_err(|e| log::warn!("no job object for the chat engine: {e}"))
        .ok()?;
    job.assign_process(child.as_raw_handle() as isize)
        .map_err(|e| log::warn!("could not add the chat engine to the job object: {e}"))
        .ok()?;
    Some(job)
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn open_log(path: &Path) -> Result<File> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

fn start_time(system: &System, pid: u32) -> Option<u64> {
    system
        .process(Pid::from_u32(pid))
        .map(sysinfo::Process::start_time)
}

fn save_pid(paths: &AppPaths, pid: u32) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let Some(start_time) = start_time(&system, pid) else {
        return;
    };
    let record = PidRecord { pid, start_time };
    if let Ok(json) = serde_json::to_vec(&record)
        && let Err(e) = std::fs::write(paths.engine_pids(), json)
    {
        log::warn!("could not save the engine process ID: {e}");
    }
}

/// The process ID of an engine that an earlier run started and did not stop.
/// The process must still be `ollama.exe` with the saved start time, so a
/// reused process ID, or an Ollama that the user started, never matches.
/// While this app runs an engine, the saved record is that engine, so call
/// this only when this run has not started one.
pub fn leftover(paths: &AppPaths) -> Option<u32> {
    let bytes = std::fs::read(paths.engine_pids()).ok()?;
    let record = serde_json::from_slice::<PidRecord>(&bytes).ok()?;
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let process = system.process(Pid::from_u32(record.pid))?;
    let is_ollama = process.name().eq_ignore_ascii_case("ollama.exe")
        || process.name().eq_ignore_ascii_case("ollama");
    (is_ollama && process.start_time() == record.start_time).then_some(record.pid)
}

/// Stops an engine that an earlier run started and did not stop (see `leftover`).
pub fn stop_leftover(paths: &AppPaths) {
    if let Some(pid) = leftover(paths) {
        log::warn!("stopping a chat engine left by an earlier run: pid {pid}");
        kill_tree(pid);
    }
    let _ = std::fs::remove_file(paths.engine_pids());
}

/// Kills a process and its direct children (the model runners).
fn kill_tree(pid: u32) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let root = Pid::from_u32(pid);
    for process in system.processes().values() {
        if process.parent() == Some(root) {
            process.kill();
        }
    }
    if let Some(process) = system.process(root) {
        process.kill();
    }
}

/// Error for a missing engine binary, in plain words.
pub fn missing_engine() -> Error {
    Error::Engine("the chat engine (Ollama) is not installed".into())
}
