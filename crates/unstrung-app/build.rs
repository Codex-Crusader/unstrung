//! Each app command gets its own permission, so the capability file is the full list
//! of what the window may call.

const COMMANDS: &[&str] = &[
    "get_status",
    "open_chat",
    "send_message",
    "retry_reply",
    "stop_reply",
    "restart_engine",
    "self_test_mode",
    "self_test_report",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attributes) {
        eprintln!("tauri build step failed: {e:#}");
        std::process::exit(1);
    }
}
