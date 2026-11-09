//! The chat commands that the UI can call. Each one checks its input and
//! returns errors in plain words, with the technical detail apart.

use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use unstrung_core::Error;
use unstrung_core::catalog::Advanced;
use unstrung_core::chat::{ChatView, DeleteUndo, Progress, RebuiltSummary, RegenMode, ReplyResult};
use unstrung_core::db::{
    Character, ChatSummary, DeletedChat, Kept, Memory, Message, Persona, RecentChat,
};
use unstrung_core::engine::EngineStatus;
use unstrung_core::models::ModelsPage;
use unstrung_core::presets::StyleSettings;

mod backup;
mod chats;
mod models;
mod replies;

pub use backup::*;
pub use chats::*;
pub use models::*;
pub use replies::*;

#[derive(Debug, Clone, Serialize)]
pub struct UiError {
    pub message: String,
    pub detail: String,
    /// A tip that the page shows under the error: "antivirus", or
    /// "password" when an import needs the backup password.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<&'static str>,
}

/// How many "file in use" errors this run saw. From the second one, the
/// error carries the antivirus tip, because real-time scanners lock files.
static FILE_IN_USE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A file that another program holds open (Windows sharing or lock violation).
fn file_in_use(e: &Error) -> bool {
    match e {
        Error::Io(io) => matches!(io.raw_os_error(), Some(32 | 33)),
        Error::Engine(text) => {
            let text = text.to_lowercase();
            text.contains("being used by another process") || text.contains("sharing violation")
        }
        _ => false,
    }
}

impl From<Error> for UiError {
    fn from(e: Error) -> Self {
        if let Error::Rejected { message, why } = &e {
            log::warn!("command failed: {e}");
            return Self {
                message: sentence(message),
                detail: why.clone(),
                tip: None,
            };
        }
        if matches!(e, Error::PasswordNeeded | Error::WrongPassword) {
            return Self {
                message: if matches!(e, Error::WrongPassword) {
                    "The password is wrong. Try again."
                } else {
                    "This backup has a password. Type it, then click Open."
                }
                .to_owned(),
                detail: e.to_string(),
                tip: Some("password"),
            };
        }
        if file_in_use(&e) {
            log::warn!("command failed, file in use: {e}");
            let seen = FILE_IN_USE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            return Self {
                message: "Another program is using a file that Unstrung needs. Wait a moment, then try again."
                    .to_owned(),
                detail: e.to_string(),
                tip: (seen >= 2).then_some("antivirus"),
            };
        }
        let message = match &e {
            Error::NotReady => {
                "The chat engine is not ready yet. Wait until the status bar says Ready, then try again."
                    .to_owned()
            }
            Error::Busy => {
                "A reply, a picture or a Looks suggestion is still being made. Wait, or press Stop.".to_owned()
            }
            Error::Timeout(secs) => format!(
                "No reply after {secs} seconds. Try again, or pick a smaller model on the Models screen."
            ),
            Error::Invalid(text) => sentence(text),
            Error::Rejected { message, .. } => sentence(message),
            Error::NotFound(what) => format!("This {what} no longer exists. Go back and open it again."),
            Error::Cancelled => "Stopped.".to_owned(),
            Error::NoSpace { need } => format!(
                "Not enough space. You need {} more. Free some space, or pick a folder on another drive.",
                gigabytes(*need)
            ),
            Error::Offline(_) => {
                "No internet connection. Connect, then press Retry. The download goes on from where it stopped."
                    .to_owned()
            }
            Error::Engine(text) if engine_out_of_memory(text) => {
                "This model is too big for your graphics card. Pick a smaller model on the Models screen."
                    .to_owned()
            }
            Error::Engine(text) if model_file_damaged(text) => {
                "The model file is missing or damaged. Download it again on the Models screen."
                    .to_owned()
            }
            Error::Engine(_) | Error::Http(_) | Error::Json(_) => {
                "The chat engine did not answer. Try again.".to_owned()
            }
            // Handled above, with the password tip.
            Error::PasswordNeeded | Error::WrongPassword => e.to_string(),
            Error::Damaged(_) => {
                "Your chats file is damaged. Open Unstrung in safe start and run Repair.".to_owned()
            }
            Error::KeyLost(_) => {
                "The key that protects your chats and pictures did not open. Your data is still there. Sign in to Windows as the user who made it, then start Unstrung again."
                    .to_owned()
            }
            Error::Db(_) | Error::Migrate(_) | Error::Io(_) => {
                "Unstrung could not read or save your data. Try again. If it fails again, restart Unstrung."
                    .to_owned()
            }
        };
        log::warn!("command failed: {e}");
        Self {
            message,
            detail: e.to_string(),
            tip: None,
        }
    }
}

/// Ollama's words when a model does not fit in memory.
fn engine_out_of_memory(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "out of memory",
        "requires more system memory",
        "insufficient memory",
        "cudamalloc failed",
        "unable to allocate",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Ollama's words when a model file is gone or broken.
fn model_file_damaged(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "invalid file magic",
        "unable to load model",
        "failed to load model",
        "file does not exist",
        "no such file",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Rounded up, so a small shortfall never shows as "0.0 GB".
pub fn gigabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let gb = bytes as f64 / 1_073_741_824.0;
    format!("{:.1} GB", (gb * 10.0).ceil() / 10.0)
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub type CmdResult<T> = Result<T, UiError>;

pub fn emit<T: tauri::ipc::IpcResponse>(channel: &Channel<T>, event: T) {
    if let Err(e) = channel.send(event) {
        log::warn!("could not send a reply event to the window: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tauri rejects a command that has no permission ("not allowed by
    /// ACL"), and the core tests never go through Tauri. So every command
    /// the app registers must be in build.rs and in the capability file.
    #[test]
    fn every_registered_command_has_a_permission() {
        let main = include_str!("../main.rs");
        let start = main.find("generate_handler![").unwrap();
        let block = &main[start..start + main[start..].find("])").unwrap()];
        let registered: std::collections::BTreeSet<&str> = block
            .split(',')
            .filter_map(|item| item.trim().rsplit("::").next())
            .filter(|name| !name.is_empty() && !name.contains('['))
            .collect();
        let build = include_str!("../../build.rs");
        let list = &build[build.find("COMMANDS").unwrap()..build.find("];").unwrap()];
        let listed: std::collections::BTreeSet<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(registered, listed, "build.rs COMMANDS");
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../../capabilities/default.json")).unwrap();
        let allowed: std::collections::BTreeSet<String> = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str()?.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        for name in &registered {
            assert!(allowed.contains(*name), "no allow-{name} in default.json");
        }
    }

    #[test]
    fn a_locked_file_gets_the_antivirus_tip_the_second_time() {
        let locked = || Error::Io(std::io::Error::from_raw_os_error(32));
        assert!(file_in_use(&locked()));
        assert!(file_in_use(&Error::Engine(
            "open blob: The process cannot access the file because it is being used by another process.".into()
        )));
        assert!(!file_in_use(&Error::Io(std::io::Error::from_raw_os_error(
            5
        ))));
        let _ = UiError::from(locked());
        let second = UiError::from(locked());
        assert_eq!(second.tip, Some("antivirus"));
        assert!(UiError::from(Error::Busy).tip.is_none());
    }

    #[test]
    fn rejected_file_keeps_the_reason_in_details() {
        let e = UiError::from(Error::Rejected {
            message: "the picture could not be read. Use a PNG, JPEG or WebP picture".into(),
            why: "unexpected end of file".into(),
        });
        assert_eq!(
            e.message,
            "The picture could not be read. Use a PNG, JPEG or WebP picture."
        );
        assert_eq!(e.detail, "unexpected end of file");
        let e = UiError::from(Error::Timeout(60));
        assert!(
            e.message
                .starts_with("No reply after 60 seconds. Try again")
        );
    }
}
