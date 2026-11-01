//! Picture commands and events: setup, the model choice, painting and the gallery.

use crate::commands::CmdResult;
use crate::state::AppState;
use tauri::{Emitter, Manager, State};
use unstrung_core::avatar::{self, Staged};
use unstrung_core::db::{GalleryImage, Image};
use unstrung_core::pictures::service::Page;

pub const DOWNLOAD_EVENT: &str = "picture-download";
pub const PAINTING_EVENT: &str = "picture-painting";

#[tauri::command]
pub async fn pictures_page(state: State<'_, AppState>) -> CmdResult<Page> {
    Ok(state.pictures.page().await?)
}

#[tauri::command]
pub fn download_picture_model(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.pictures.download(&id)?)
}

#[tauri::command]
pub fn stop_picture_download(state: State<'_, AppState>) {
    state.pictures.stop_download();
}

#[tauri::command]
pub async fn select_picture_model(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.pictures.select(&id).await?)
}

#[tauri::command]
pub async fn remove_picture_model(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.pictures.remove(&id).await?)
}

/// Makes a picture of a message (the latest one when `message_id` is empty).
/// Stop (`stop_reply`) cancels it, and then nothing comes back.
#[tauri::command]
pub async fn paint_picture(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: Option<i64>,
    wish: String,
) -> CmdResult<Option<Image>> {
    let wish: String = wish.chars().take(500).collect();
    match state.pictures.paint(chat_id, message_id, &wish).await {
        Ok(image) => Ok(Some(image)),
        Err(unstrung_core::error::Error::Cancelled) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// A variation of a gallery picture: a new seed and another framing.
#[tauri::command]
pub async fn repaint_picture(state: State<'_, AppState>, id: i64) -> CmdResult<Option<Image>> {
    match state.pictures.repaint(id).await {
        Ok(image) => Ok(Some(image)),
        Err(unstrung_core::error::Error::Cancelled) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Stages a gallery picture as a new profile picture, like an upload. The
/// creator then shows the crop.
#[tauri::command]
pub async fn stage_picture(state: State<'_, AppState>, id: i64) -> CmdResult<Staged> {
    let bytes = tokio::fs::read(state.pictures.file(id).await?)
        .await
        .map_err(unstrung_core::error::Error::from)?;
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || avatar::stage(&paths, &bytes))
        .await
        .map_err(|e| unstrung_core::error::Error::Engine(e.to_string()))?
        .map_err(Into::into)
}

/// Saves a copy of a picture where the user picks. False when the user
/// closes the dialog.
#[tauri::command]
pub async fn export_picture(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> CmdResult<bool> {
    let from = state.pictures.file(id).await?;
    let safe: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .take(60)
        .collect();
    let file = format!(
        "{} {id}.png",
        if safe.trim().is_empty() {
            "Picture"
        } else {
            safe.trim()
        }
    );
    let Some(to) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_file_name(file)
        .add_filter("PNG", &["png"])
        .save_file()
        .await
    else {
        return Ok(false);
    };
    tokio::fs::copy(from, to.path())
        .await
        .map_err(unstrung_core::error::Error::from)?;
    Ok(true)
}

#[tauri::command]
pub async fn chat_pictures(state: State<'_, AppState>, chat_id: i64) -> CmdResult<Vec<Image>> {
    Ok(state.chats.db().chat_images(chat_id).await?)
}

#[tauri::command]
pub async fn gallery(state: State<'_, AppState>) -> CmdResult<Vec<GalleryImage>> {
    Ok(state.chats.db().gallery().await?)
}

#[tauri::command]
pub async fn delete_picture(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(state.pictures.delete(id).await?)
}

/// Sends download progress and painting progress to the window.
pub fn forward_progress(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut downloads = state.pictures.subscribe_download();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while downloads.changed().await.is_ok() {
            let current = downloads.borrow_and_update().clone();
            if let Err(e) = handle.emit(DOWNLOAD_EVENT, current) {
                log::warn!("could not send the picture download progress: {e}");
            }
        }
    });
    let mut painting = state.pictures.subscribe_painting();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while painting.changed().await.is_ok() {
            let current = painting.borrow_and_update().clone();
            if let Err(e) = handle.emit(PAINTING_EVENT, current) {
                log::warn!("could not send the picture progress: {e}");
            }
        }
    });
}
