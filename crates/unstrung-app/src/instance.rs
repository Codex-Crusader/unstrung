//! One Unstrung process per app folder, so two processes never open the same
//! database. The folder lock (`start::lock`) decides which process runs. A
//! second start asks the running one, over a named pipe, to show its window,
//! and then exits. The pipe name comes from the app folder, so a test run with
//! its own folder (`UNSTRUNG_HOME`) never meets the user's app.

use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::net::windows::named_pipe::ServerOptions;
use unstrung_core::paths::AppPaths;
use unstrung_core::start;

/// Added by "Start normally": the old process still holds the lock while it
/// closes, so the new one waits for the lock instead of waking the old window.
pub const RESTART_ARG: &str = "--restart";
const RESTART_WAIT: Duration = Duration::from_secs(20);
/// How long a second start tries to reach a window that is still opening.
const WAKE_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);

pub enum Claim {
    /// This process owns the folder. Keep the lock until the process ends.
    /// `None` when the lock file could not be made: the app runs anyway,
    /// because blocking the user is worse.
    First(Option<File>),
    /// Another process owns the folder.
    Second,
}

pub fn claim(paths: &AppPaths, restart: bool) -> Claim {
    let deadline = Instant::now()
        + if restart {
            RESTART_WAIT
        } else {
            Duration::ZERO
        };
    loop {
        match start::lock(paths) {
            Ok(Some(lock)) => return Claim::First(Some(lock)),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => return Claim::Second,
            Err(e) => {
                eprintln!("no app folder lock: {e}");
                return Claim::First(None);
            }
        }
    }
}

fn pipe_name(paths: &AppPaths) -> String {
    let mut hasher = DefaultHasher::new();
    paths
        .root()
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    format!(r"\\.\pipe\unstrung-{:016x}", hasher.finish())
}

/// In a second process: asks the running one to show its window. False when
/// no window answered in time, for example when the other process hangs.
pub fn wake_first(paths: &AppPaths) -> bool {
    let name = pipe_name(paths);
    let deadline = Instant::now() + WAKE_WAIT;
    loop {
        let sent = std::fs::OpenOptions::new()
            .write(true)
            .open(&name)
            .and_then(|mut pipe| pipe.write_all(b"show"));
        if sent.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// In the first process: shows the main window each time a second start asks.
/// `first_pipe_instance` makes sure that this process owns the name, so no
/// other program can take it first.
pub fn serve(app: &AppHandle, paths: &AppPaths) {
    let name = pipe_name(paths);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(server) => server,
            Err(e) => {
                log::warn!("a second start cannot show this window: {e}");
                return;
            }
        };
        loop {
            if let Err(e) = server.connect().await {
                log::warn!("second start pipe: {e}");
            }
            // The next instance first, so a quick third start finds one.
            server = match ServerOptions::new().create(&name) {
                Ok(next) => next,
                Err(e) => {
                    log::warn!("second start pipe: {e}");
                    return;
                }
            };
            if let Some(window) = app.get_webview_window("main") {
                log::info!("a second start asked to show the window");
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    });
}
