//! The commands that the UI can call. Each one checks its input and returns
//! errors in plain words, with the technical detail apart.

use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use unstrung_core::Error;
use unstrung_core::chat::ChatView;
use unstrung_core::db::Message;
use unstrung_core::engine::EngineStatus;

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

type CmdResult<T> = Result<T, UiError>;

/// Events for one reply, sent through a channel in order.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyEvent {
    UserSaved { message: Message },
    Text { text: String },
    Done { message: Option<Message> },
    Failed { error: UiError },
}

fn emit(channel: &Channel<ReplyEvent>, event: ReplyEvent) {
    if let Err(e) = channel.send(event) {
        log::warn!("could not send a reply event to the window: {e}");
    }
}

fn finish(channel: &Channel<ReplyEvent>, result: unstrung_core::Result<Option<Message>>) {
    match result {
        Ok(message) => emit(channel, ReplyEvent::Done { message }),
        Err(e) => emit(channel, ReplyEvent::Failed { error: e.into() }),
    }
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

/// Phase 1 has one character, so the UI opens its latest chat.
#[tauri::command]
pub async fn open_chat(state: State<'_, AppState>) -> CmdResult<ChatView> {
    Ok(state.chats.open_chat(state.default_character).await?)
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
        .send(
            chat_id,
            &text,
            |message| {
                emit(
                    &on_event,
                    ReplyEvent::UserSaved {
                        message: message.clone(),
                    },
                );
            },
            |text| {
                emit(
                    &on_event,
                    ReplyEvent::Text {
                        text: text.to_owned(),
                    },
                );
            },
        )
        .await;
    finish(&on_event, result);
    Ok(())
}

#[tauri::command]
pub async fn retry_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .retry(chat_id, |text| {
            emit(
                &on_event,
                ReplyEvent::Text {
                    text: text.to_owned(),
                },
            );
        })
        .await;
    finish(&on_event, result);
    Ok(())
}

#[tauri::command]
pub fn stop_reply(state: State<'_, AppState>) {
    state.chats.stop();
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move { engine.restart().await });
    Ok(())
}
