//! One Unstrung process per app folder, so two processes never open the same
//! database. The folder lock (`start::lock`) decides which process runs. A
//! second start tells the running one, over a named pipe, what the user wants,
//! and then exits. The pipe name comes from the app folder, so a test run with
//! its own folder (`UNSTRUNG_HOME`) never meets the user's app.

use crate::safe::{SAFE_START_ARG, StartInfo};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::ServerOptions;
use unstrung_core::paths::AppPaths;
use unstrung_core::start;

/// Added to every relaunch: the old process still holds the lock while it
/// closes, so the new one waits for the lock instead of messaging the old one.
pub const RESTART_ARG: &str = "--restart";
const RESTART_WAIT: Duration = Duration::from_secs(20);
/// How long a second start tries to reach a window that is still opening.
const WAKE_WAIT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);
const READ_WAIT: Duration = Duration::from_secs(2);

pub enum Claim {
    /// This process owns the folder. Keep the lock until the process ends.
    /// `None` when the lock file could not be made: the app runs anyway,
    /// because blocking the user is worse.
    First(Option<File>),
    /// Another process owns the folder.
    Second,
}

/// What a second start asks of the running process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wish {
    /// Show the window (a normal second start).
    Show,
    /// Restart in safe start (the "Unstrung (Safe start)" shortcut).
    Safe,
}

impl Wish {
    fn message(self) -> &'static [u8] {
        match self {
            Self::Show => b"show",
            Self::Safe => b"safe",
        }
    }

    fn parse(message: &[u8]) -> Self {
        if message == Self::Safe.message() {
            Self::Safe
        } else {
            Self::Show
        }
    }
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

/// In a second process: sends the wish to the running one. False when no
/// window answered in time, for example when the other process hangs.
pub fn send(paths: &AppPaths, wish: Wish) -> bool {
    let name = pipe_name(paths);
    let deadline = Instant::now() + WAKE_WAIT;
    loop {
        let sent = std::fs::OpenOptions::new()
            .write(true)
            .open(&name)
            .and_then(|mut pipe| pipe.write_all(wish.message()));
        if sent.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// In the first process: acts on each second start. `first_pipe_instance`
/// makes sure that this process owns the name, so no other program takes it.
pub fn serve(app: &AppHandle, paths: &AppPaths) {
    let name = pipe_name(paths);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
            Ok(server) => server,
            Err(e) => {
                log::warn!("a second start cannot reach this window: {e}");
                return;
            }
        };
        loop {
            if let Err(e) = server.connect().await {
                log::warn!("second start pipe: {e}");
            }
            // Read before the next step, so the sender's write never fails
            // and the sender never retries.
            let mut buf = [0u8; 8];
            let read = tokio::time::timeout(READ_WAIT, server.read(&mut buf)).await;
            let wish = match read {
                Ok(Ok(n)) => Wish::parse(&buf[..n]),
                _ => Wish::Show,
            };
            // The next instance first, so a quick third start finds one.
            server = match ServerOptions::new().create(&name) {
                Ok(next) => next,
                Err(e) => {
                    log::warn!("second start pipe: {e}");
                    return;
                }
            };
            act(&app, wish);
        }
    });
}

fn act(app: &AppHandle, wish: Wish) {
    let in_safe_start = app
        .try_state::<StartInfo>()
        .is_some_and(|info| info.safe.is_some());
    if wish == Wish::Safe && !in_safe_start {
        // No question first: the user chose the safe start shortcut, and a
        // broken window may not be able to show a question. Chats are saved
        // as they happen, so a restart loses an unsent draft and cuts off a
        // reply that is still being written.
        log::warn!("a second start asked for safe start: restarting");
        match relaunch(app, true) {
            Ok(()) => return,
            Err(e) => log::error!("could not restart in safe start: {e}"),
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        log::info!("a second start asked to show the window");
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Starts a new process with this one's arguments and closes this one. The
/// new process waits for the lock (`RESTART_ARG`) and opens in safe start
/// only when `safe` is true.
pub fn relaunch(app: &AppHandle, safe: bool) -> unstrung_core::Result<()> {
    let mut args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != SAFE_START_ARG && a != RESTART_ARG)
        .collect();
    if safe {
        args.push(SAFE_START_ARG.to_owned());
    }
    args.push(RESTART_ARG.to_owned());
    std::process::Command::new(std::env::current_exe()?)
        .args(args)
        .spawn()?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_safe_message_restarts() {
        assert_eq!(Wish::parse(b"safe"), Wish::Safe);
        assert_eq!(Wish::parse(b"show"), Wish::Show);
        assert_eq!(Wish::parse(b"saf"), Wish::Show);
        assert_eq!(Wish::parse(b""), Wish::Show);
    }

    #[test]
    fn the_pipe_name_follows_the_folder() {
        let a = AppPaths::at("C:\\Users\\A\\AppData\\Local\\Unstrung".into());
        let b = AppPaths::at("c:\\users\\a\\appdata\\local\\unstrung".into());
        let c = AppPaths::at("C:\\test\\home".into());
        assert_eq!(pipe_name(&a), pipe_name(&b));
        assert_ne!(pipe_name(&a), pipe_name(&c));
    }
}
