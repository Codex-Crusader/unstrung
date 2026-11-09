//! App updates from GitHub Releases (`plugins.updater` in tauri.conf.json).
//! The app checks once a while after start and when the user asks, unless
//! the user turned the check off (meta key `update_check`). It downloads only
//! when the user clicks Update. The updater installs only a file signed with
//! the public key in tauri.conf.json, for the version that it announces.
//! The window has no updater permission: only these commands reach it.

use crate::commands::{CmdResult, UiError};
use crate::state::AppState;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const META_KEY: &str = "update_check";
/// The check after start waits, so it does not slow the start.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);
/// The app can stay open for weeks, so it checks again each day.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct Available {
    pub version: String,
    /// The release notes, as the release has them.
    pub notes: String,
}

impl From<&Update> for Available {
    fn from(u: &Update) -> Self {
        Self {
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        }
    }
}

fn failed(message: &str, e: &tauri_plugin_updater::Error) -> UiError {
    log::info!("{message} {e}");
    UiError {
        message: message.to_owned(),
        detail: e.to_string(),
        tip: None,
    }
}

const CHECK_FAILED: &str =
    "Unstrung could not check for updates. Check the internet connection, then try again.";
const DOWNLOAD_FAILED: &str = "The update did not download. Your chats are as they were. Check the internet connection, then try again.";

async fn checks_on(state: &AppState) -> bool {
    state
        .chats
        .db()
        .meta(META_KEY)
        .await
        .ok()
        .flatten()
        .as_deref()
        != Some("off")
}

async fn find(app: &AppHandle) -> tauri_plugin_updater::Result<Option<Update>> {
    app.updater()?.check().await
}

/// The check after start, then once a day until an update is found. A
/// failed check is quiet, because Unstrung works offline and there is
/// nothing to tell.
pub fn check_after_start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            // Settings can turn the check on or off while the app runs.
            if checks_on(&state).await {
                match find(&app).await {
                    Ok(Some(update)) => {
                        log::info!("Unstrung {} is ready to install", update.version);
                        let _ = app.emit("update-available", Available::from(&update));
                        return;
                    }
                    Ok(None) => log::info!("Unstrung is up to date"),
                    Err(e) => log::info!("the update check failed: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> CmdResult<Option<Available>> {
    match find(&app).await {
        Ok(update) => Ok(update.as_ref().map(Available::from)),
        Err(e) => Err(failed(CHECK_FAILED, &e)),
    }
}

/// Downloads the update, copies the chats, closes the app the normal way,
/// and starts the installer, which opens the new version when it is done.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    // The app quits for the installer, which would stop these halfway.
    state.chats.idle()?;
    if state.models.downloading() || state.setup.busy() {
        return Err(unstrung_core::error::Error::Busy.into());
    }
    let _pictures = state.pictures.hold()?;
    let update = match find(&app).await {
        Ok(Some(update)) => update,
        Ok(None) => {
            return Err(UiError {
                message: "There is no update now. Unstrung is up to date.".into(),
                detail: String::new(),
                tip: None,
            });
        }
        Err(e) => return Err(failed(CHECK_FAILED, &e)),
    };
    let mut got: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                got += chunk as u64;
                let _ = app.emit("update-progress", (got, total));
            },
            || {},
        )
        .await
        .map_err(|e| failed(DOWNLOAD_FAILED, &e))?;
    // Safe start can put this copy back if the new version goes wrong.
    let copy = state
        .chats
        .db()
        .update_snapshot(&state.paths.database())
        .await?;
    log::info!(
        "installing Unstrung {}, chats copied to {}",
        update.version,
        copy.display()
    );
    // On Windows the install ends this process at once, so the normal
    // shutdown of the run loop never runs: it runs here.
    state.chats.shut_down();
    state.engine.stop().await;
    state.chats.db().close().await;
    if let Err(e) = update.install(bytes) {
        log::error!("the installer did not start: {e}");
        // The database is closed: a restart opens it again.
        app.restart();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_update_check(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(checks_on(&state).await)
}

#[tauri::command]
pub async fn set_update_check(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    log::info!("update check: {}", if on { "on" } else { "off" });
    Ok(state
        .chats
        .db()
        .set_meta(META_KEY, if on { "on" } else { "off" })
        .await?)
}
