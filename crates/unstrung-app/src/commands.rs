//! The chat commands that the UI can call. Each one checks its input and
//! returns errors in plain words, with the technical detail apart.

use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use unstrung_core::Error;
use unstrung_core::chat::{ChatView, DeleteUndo, Progress, RegenMode, ReplyResult};
use unstrung_core::db::{Character, ChatSummary, Message, Persona};
use unstrung_core::engine::EngineStatus;
use unstrung_core::presets::StyleSettings;

#[derive(Debug, Clone, Serialize)]
pub struct UiError {
    pub message: String,
    pub detail: String,
}

impl From<Error> for UiError {
    fn from(e: Error) -> Self {
        let message = match &e {
            Error::NotReady => "The chat engine is not ready yet.".to_owned(),
            Error::Busy => "A reply is still being written. Wait, or press Stop.".to_owned(),
            Error::Timeout(secs) => format!("No reply after {secs} seconds."),
            Error::Invalid(text) => sentence(text),
            Error::NotFound(what) => format!("This {what} does not exist."),
            Error::Engine(_) | Error::Http(_) | Error::Json(_) => "The reply failed.".to_owned(),
            Error::Db(_) | Error::Migrate(_) | Error::Io(_) => {
                "Your data could not be saved.".to_owned()
            }
        };
        log::warn!("command failed: {e}");
        Self {
            message,
            detail: e.to_string(),
        }
    }
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub type CmdResult<T> = Result<T, UiError>;

/// Events for one reply, sent through a channel in order.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyEvent {
    UserSaved {
        message: Message,
    },
    Text {
        text: String,
    },
    /// The guard rejected the text so far. The UI clears it and waits for new text.
    Restart,
    Done {
        view: Box<ChatView>,
        hint: Option<&'static str>,
    },
    Failed {
        error: UiError,
    },
}

pub fn emit(channel: &Channel<ReplyEvent>, event: ReplyEvent) {
    if let Err(e) = channel.send(event) {
        log::warn!("could not send a reply event to the window: {e}");
    }
}

pub fn progress(channel: &Channel<ReplyEvent>) -> impl FnMut(Progress<'_>) + '_ {
    move |p| match p {
        Progress::Text(text) => emit(
            channel,
            ReplyEvent::Text {
                text: text.to_owned(),
            },
        ),
        Progress::Restart => emit(channel, ReplyEvent::Restart),
    }
}

fn user_saved(channel: &Channel<ReplyEvent>) -> impl FnOnce(&Message) + '_ {
    move |message| {
        emit(
            channel,
            ReplyEvent::UserSaved {
                message: message.clone(),
            },
        );
    }
}

/// Sends the end of a reply. The view is sent after an error too, because a
/// user message can be saved before the reply fails.
async fn finish(
    state: &AppState,
    chat_id: i64,
    channel: &Channel<ReplyEvent>,
    result: unstrung_core::Result<ReplyResult>,
) {
    let event = match result {
        Ok(reply) => match state.chats.view(chat_id).await {
            Ok(view) => ReplyEvent::Done {
                view: Box::new(view),
                hint: reply.hint,
            },
            Err(e) => ReplyEvent::Failed { error: e.into() },
        },
        Err(e) => ReplyEvent::Failed { error: e.into() },
    };
    emit(channel, event);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

#[tauri::command]
pub async fn list_characters(state: State<'_, AppState>) -> CmdResult<Vec<Character>> {
    Ok(state.chats.db().characters().await?)
}

#[tauri::command]
pub async fn list_personas(state: State<'_, AppState>) -> CmdResult<Vec<Persona>> {
    Ok(state.chats.db().personas().await?)
}

/// The latest chat with a character, or a new one.
#[tauri::command]
pub async fn open_character(state: State<'_, AppState>, character_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.open_latest(character_id).await?)
}

#[tauri::command]
pub async fn open_chat(state: State<'_, AppState>, chat_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.view(chat_id).await?)
}

#[tauri::command]
pub async fn new_chat(
    state: State<'_, AppState>,
    character_id: i64,
    persona_id: Option<i64>,
) -> CmdResult<ChatView> {
    Ok(state.chats.new_chat(character_id, persona_id).await?)
}

#[tauri::command]
pub async fn list_chats(
    state: State<'_, AppState>,
    character_id: i64,
) -> CmdResult<Vec<ChatSummary>> {
    Ok(state.chats.chats(character_id).await?)
}

#[tauri::command]
pub async fn delete_chat(state: State<'_, AppState>, chat_id: i64) -> CmdResult<()> {
    Ok(state.chats.delete_chat(chat_id).await?)
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    chat_id: i64,
    text: String,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .send(chat_id, &text, user_saved(&on_event), progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

/// A user message gets a new version and a new reply.
#[tauri::command]
pub async fn edit_message(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    text: String,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .edit_and_resend(
            chat_id,
            message_id,
            &text,
            user_saved(&on_event),
            progress(&on_event),
        )
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

/// A reply is fixed by hand, in place.
#[tauri::command]
pub async fn edit_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    text: String,
) -> CmdResult<ChatView> {
    Ok(state
        .chats
        .edit_reply_text(chat_id, message_id, &text)
        .await?)
}

#[tauri::command]
pub async fn regenerate(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    mode: RegenMode,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .regenerate(chat_id, message_id, mode, progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub async fn continue_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .continue_reply(chat_id, progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub async fn retry_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state.chats.retry(chat_id, progress(&on_event)).await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub fn stop_reply(state: State<'_, AppState>) {
    state.chats.stop();
}

/// "Rewind to here": later messages stay as a hidden branch.
#[tauri::command]
pub async fn rewind(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_head(chat_id, message_id).await?)
}

/// Shows another version of a message.
#[tauri::command]
pub async fn show_branch(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.show_branch(chat_id, message_id).await?)
}

#[derive(Serialize)]
pub struct Deleted {
    view: ChatView,
    undo: DeleteUndo,
}

#[tauri::command]
pub async fn delete_message(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<Deleted> {
    let (view, undo) = state.chats.delete_message(chat_id, message_id).await?;
    Ok(Deleted { view, undo })
}

#[tauri::command]
pub async fn undo_delete(
    state: State<'_, AppState>,
    chat_id: i64,
    undo: DeleteUndo,
) -> CmdResult<ChatView> {
    Ok(state.chats.undo_delete(chat_id, &undo).await?)
}

/// Copies the chat up to a message into a new chat.
#[tauri::command]
pub async fn branch_chat(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.branch(chat_id, message_id).await?)
}

#[tauri::command]
pub async fn set_chat_persona(
    state: State<'_, AppState>,
    chat_id: i64,
    persona_id: Option<i64>,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_persona(chat_id, persona_id).await?)
}

#[tauri::command]
pub async fn set_chat_tone(
    state: State<'_, AppState>,
    chat_id: i64,
    tone: String,
    custom: String,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_tone(chat_id, &tone, &custom).await?)
}

#[tauri::command]
pub async fn set_chat_style(
    state: State<'_, AppState>,
    chat_id: i64,
    style: String,
    custom: Option<StyleSettings>,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_style(chat_id, &style, custom).await?)
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move { engine.restart().await });
    Ok(())
}
