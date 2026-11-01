//! A Windows notification when a long job ends while the window is minimized
//! or in the background. The user can turn it off (meta key `notify_done`).

use crate::state::AppState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};

pub const META_KEY: &str = "notify_done";

#[derive(Clone, Copy)]
pub enum Job {
    Engine,
    Model,
    Pictures,
}

impl Job {
    fn text(self) -> &'static str {
        match self {
            Self::Engine => "The chat engine is installed.",
            Self::Model => "The model download finished. Your characters are ready.",
            Self::Pictures => "Pictures are ready. Press Picture this in a chat.",
        }
    }
}

/// Shows the notification unless the user is looking at the window, or
/// turned notifications off.
pub fn job_done(app: &AppHandle, job: Job) {
    let enabled = app
        .try_state::<AppState>()
        .is_some_and(|s| s.notify.load(Ordering::Relaxed));
    let window = app.get_webview_window("main");
    let watching = window.as_ref().is_some_and(|w| {
        w.is_visible().unwrap_or(false)
            && w.is_focused().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
    });
    if !enabled || watching {
        return;
    }
    show(app, job.text());
}

#[cfg(windows)]
fn show(app: &AppHandle, text: &str) {
    use tauri_winrt_notification::Toast;
    // The installer's Start menu shortcut carries the app ID. A debug build
    // has no shortcut, so it borrows PowerShell's ID, which Windows knows.
    let id = if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID.to_owned()
    } else {
        app.config().identifier.clone()
    };
    if let Err(e) = Toast::new(&id).title("Unstrung").text1(text).show() {
        log::warn!("could not show a notification: {e}");
    }
}

#[cfg(not(windows))]
fn show(_app: &AppHandle, _text: &str) {}

#[tauri::command]
pub async fn set_notify(
    state: tauri::State<'_, AppState>,
    on: bool,
) -> crate::commands::CmdResult<()> {
    state
        .chats
        .db()
        .set_meta(META_KEY, if on { "on" } else { "off" })
        .await?;
    state.notify.store(on, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn get_notify(state: tauri::State<'_, AppState>) -> bool {
    state.notify.load(Ordering::Relaxed)
}
