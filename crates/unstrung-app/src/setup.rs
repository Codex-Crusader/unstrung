//! First run and the engine download: the storage step, the folder picker,
//! the engine install, and the flag that ends the welcome flow.

use crate::commands::{CmdResult, UiError};
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use unstrung_core::Error;
use unstrung_core::db::FIRST_RUN_KEY;
use unstrung_core::engine_setup::{self, Progress};
use unstrung_core::location::{self, FolderCheck, Location, Purpose};

pub const ENGINE_SETUP_EVENT: &str = "engine-setup";
pub const ENGINE_SETUP_ERROR_EVENT: &str = "engine-setup-error";
/// The catalog model picked in the setup step, kept while the first run lasts.
const META_CHOICE: &str = "first_run_model";

#[derive(Serialize)]
pub struct SetupState {
    pub first_run: bool,
    /// The engine version that Unstrung installed, if any.
    pub engine_installed: Option<String>,
    pub engine_version: String,
    pub engine_download_bytes: u64,
    /// Free space that the engine install needs at its peak.
    pub engine_disk_bytes: u64,
    pub engine_progress: Option<Progress>,
    /// The model picked in the setup step, so a restart resumes the download.
    pub choice: Option<String>,
    pub app_folder: String,
    pub library: String,
    pub models: String,
    /// The default folders, for "Use the default".
    pub default_library: String,
    pub default_models: String,
}

#[tauri::command]
pub async fn setup_state(state: State<'_, AppState>) -> CmdResult<SetupState> {
    let first_run = state.chats.db().first_run_pending().await?;
    let gpu = state.models.page().await?.hardware.gpu;
    let paths = &state.paths;
    let defaults = Location::default();
    Ok(SetupState {
        first_run,
        engine_installed: engine_setup::installed_version(paths),
        engine_version: engine_setup::release().version.clone(),
        engine_download_bytes: engine_setup::download_bytes(gpu.as_ref()),
        engine_disk_bytes: engine_setup::disk_need(gpu.as_ref()),
        engine_progress: state.setup.current(),
        choice: state.chats.db().meta(META_CHOICE).await?,
        app_folder: shown(paths.root().to_path_buf()),
        library: shown(paths.library()),
        models: shown(paths.models()),
        default_library: shown(defaults.library(paths.root())),
        default_models: shown(defaults.models(paths.root())),
    })
}

fn shown(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

/// The Windows folder picker. `None` when the user closes it.
#[tauri::command]
pub async fn pick_folder(start: Option<String>) -> Option<String> {
    #[cfg(windows)]
    {
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(start) = start.filter(|s| !s.is_empty()) {
            dialog = dialog.set_directory(start);
        }
        dialog
            .pick_folder()
            .await
            .map(|f| shown(f.path().to_path_buf()))
    }
    #[cfg(not(windows))]
    {
        let _ = start;
        None
    }
}

#[tauri::command]
pub async fn check_folder(path: String, purpose: Purpose, download_bytes: u64) -> FolderCheck {
    tokio::task::spawn_blocking(move || {
        location::check(&PathBuf::from(path), purpose, download_bytes)
    })
    .await
    .unwrap_or_else(|_| location::check(&PathBuf::new(), purpose, download_bytes))
}

/// Saves the folders from the storage step. `None` keeps the default.
/// Only during the first run: moving files later is Settings > Storage.
#[tauri::command]
pub async fn set_location(
    state: State<'_, AppState>,
    library: Option<String>,
    models: Option<String>,
    download_bytes: u64,
) -> CmdResult<SetupState> {
    if !state.chats.db().first_run_pending().await? {
        return Err(
            Error::Invalid("the folders can change only during the first run".into()).into(),
        );
    }
    let pick = |path: Option<String>, purpose: Purpose, bytes: u64| -> CmdResult<Option<PathBuf>> {
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let check = location::check(&PathBuf::from(path), purpose, bytes);
        if check.blocked {
            return Err(
                Error::Invalid(format!("this folder cannot be used: {:?}", check.issues)).into(),
            );
        }
        Ok(Some(check.folder))
    };
    let library = pick(library, Purpose::Library, 0)?;
    let models = pick(models, Purpose::Models, download_bytes)?;
    if let Some(folder) = &library {
        location::mark_library(folder)?;
    }
    if let Some(folder) = &models {
        std::fs::create_dir_all(folder).map_err(Error::from)?;
    }
    let changed = state.paths.location()
        != Location {
            library: library.clone(),
            models: models.clone(),
        };
    state.paths.set_location(Location { library, models })?;
    log::info!("storage step: library and models folders saved");
    // The engine reads its models folder when it starts.
    if changed
        && !matches!(
            state.engine.status(),
            unstrung_core::engine::EngineStatus::NotInstalled
        )
    {
        state.engine.restart().await;
    }
    setup_state(state).await
}

/// Downloads and installs the engine in the background. Progress goes to
/// `ENGINE_SETUP_EVENT`; a failure goes to `ENGINE_SETUP_ERROR_EVENT`.
#[tauri::command]
pub async fn engine_install(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let gpu = state.models.page().await?.hardware.gpu;
    let setup = state.setup.clone();
    let engine = state.engine.clone();
    let models = state.models.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            setup.prepare(gpu.as_ref()).await?;
            engine.stop().await;
            let replaced = setup.replace();
            engine.start().await;
            replaced?;
            models.after_start().await
        }
        .await;
        match result {
            Ok(()) => crate::notify::job_done(&app, crate::notify::Job::Engine),
            Err(Error::Cancelled) => {}
            Err(e) => {
                log::error!("engine install failed: {e}");
                let _ = app.emit(ENGINE_SETUP_ERROR_EVENT, UiError::from(e));
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn engine_install_cancel(state: State<'_, AppState>) {
    state.setup.cancel();
}

/// Saves the model from the setup step. Only catalog chat models.
#[tauri::command]
pub async fn choose_setup(state: State<'_, AppState>, model: String) -> CmdResult<()> {
    if !unstrung_core::catalog::entry(&model).is_some_and(|e| e.kind == "chat") {
        return Err(Error::Invalid(format!("{model} is not a catalog chat model")).into());
    }
    state.chats.db().set_meta(META_CHOICE, &model).await?;
    Ok(())
}

/// Ends the welcome flow. The next start opens Home.
#[tauri::command]
pub async fn finish_first_run(state: State<'_, AppState>) -> CmdResult<()> {
    state.chats.db().set_meta(FIRST_RUN_KEY, "done").await?;
    log::info!("first run finished");
    Ok(())
}

/// Sends engine install progress to the window.
pub fn forward_progress(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let setup: Arc<_> = state.setup.clone();
    let mut progress = setup.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while progress.changed().await.is_ok() {
            let current = progress.borrow_and_update().clone();
            if let Err(e) = handle.emit(ENGINE_SETUP_EVENT, current) {
                log::warn!("could not send the engine progress: {e}");
            }
        }
    });
}
