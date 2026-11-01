//! Picture commands and events: setup, the model choice, painting and the gallery.

use crate::commands::CmdResult;
use crate::state::AppState;
use tauri::{Emitter, Manager, State};
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

/// Paints a gallery picture again: the same seed, or a new one (a variation).
#[tauri::command]
pub async fn repaint_picture(
    state: State<'_, AppState>,
    id: i64,
    same_seed: bool,
) -> CmdResult<Option<Image>> {
    match state.pictures.repaint(id, same_seed).await {
        Ok(image) => Ok(Some(image)),
        Err(unstrung_core::error::Error::Cancelled) => Ok(None),
        Err(e) => Err(e.into()),
    }
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
