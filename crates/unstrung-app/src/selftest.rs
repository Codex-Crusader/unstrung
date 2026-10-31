//! `--self-test`: the UI sends one message through the real command path,
//! reports the result, and the app exits with code 0 (pass) or 1 (fail).
//! Run it with `UNSTRUNG_HOME` set to an empty folder. Run it twice to check
//! that the history stays after a restart. The report is `logs/self-test.json`.

use crate::safe::StartInfo;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

// Long enough for a first run that installs the engine in a debug build.
const LIMIT: Duration = Duration::from_secs(900);

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub ok: bool,
    pub detail: String,
    /// Messages in the chat when the window opened.
    pub messages_at_open: u32,
    /// Messages after the reply.
    pub messages_after: u32,
    /// Pieces of streamed text.
    pub text_events: u32,
}

// Both read `StartInfo`, because safe start has no chat state.
#[tauri::command]
pub fn self_test_mode(state: State<'_, StartInfo>) -> bool {
    state.self_test
}

#[tauri::command]
pub fn self_test_report(app: AppHandle, state: State<'_, StartInfo>, report: Report) {
    if !state.self_test {
        return;
    }
    let file = state.paths.logs().join("self-test.json");
    match serde_json::to_vec_pretty(&report) {
        Ok(json) => {
            if let Err(e) = unstrung_core::files::write_atomic(&file, &json) {
                log::error!("could not write the self-test report: {e}");
            }
        }
        Err(e) => log::error!("could not encode the self-test report: {e}"),
    }
    log::info!("self-test finished: {report:?}");
    exit(&app, if report.ok { 0 } else { 1 });
}

/// Fails the self-test if the UI does not report in time.
pub fn arm_timeout(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(LIMIT).await;
        log::error!("self-test timed out after {} seconds", LIMIT.as_secs());
        exit(&app, 1);
    });
}

fn exit(app: &AppHandle, code: i32) {
    let engine = app.try_state::<AppState>().map(|s| {
        s.chats.shut_down();
        s.engine.clone()
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(engine) = engine {
            engine.stop().await;
        }
        app.exit(code);
    });
}
