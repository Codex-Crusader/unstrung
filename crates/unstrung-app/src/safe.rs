//! Safe start in the window: the start mode, the "it loaded" signal, the
//! diagnostic report and "Start normally".

use crate::commands::CmdResult;
use serde::Serialize;
use tauri::State;
use unstrung_core::paths::AppPaths;
use unstrung_core::start::{self, SafeReason};

/// How this start went. Managed in both modes, so the window can always ask.
pub struct StartInfo {
    pub paths: AppPaths,
    /// `None` is a normal start.
    pub safe: Option<SafeReason>,
    /// Why the chat data did not open, for `SafeReason::DataDidNotOpen`.
    pub error: String,
    pub self_test: bool,
}

#[derive(Serialize)]
pub struct StartView {
    pub safe: bool,
    pub reason: Option<SafeReason>,
    pub error: String,
}

#[tauri::command]
pub fn start_mode(info: State<'_, StartInfo>) -> StartView {
    StartView {
        safe: info.safe.is_some(),
        reason: info.safe,
        error: info.error.clone(),
    }
}

/// The window loaded, so this start counts as good. In safe start the count
/// stays, so the next start is a safe start again until the user picks
/// "Start normally".
#[tauri::command]
pub fn start_ok(info: State<'_, StartInfo>) -> CmdResult<()> {
    if info.safe.is_none() && !info.self_test {
        start::clear(&info.paths)?;
    }
    Ok(())
}

/// Restarts the app in normal mode.
#[tauri::command]
pub fn start_normally(app: tauri::AppHandle, info: State<'_, StartInfo>) -> CmdResult<()> {
    start::clear(&info.paths)?;
    // A restart keeps the arguments, and `--safe-start` would come back.
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != SAFE_START_ARG)
        .collect();
    let exe = std::env::current_exe().map_err(unstrung_core::Error::from)?;
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .map_err(unstrung_core::Error::from)?;
    log::info!("safe start: starting normally");
    app.exit(0);
    Ok(())
}

pub const SAFE_START_ARG: &str = "--safe-start";

/// Lines of the app log in the report.
const LOG_LINES: usize = 80;
/// Log lines that can hold chat text. The report keeps only their first part.
const CUT: &[(&str, usize)] = &[
    ("command failed: ", 2),
    ("open thread in ", 1),
    ("self-test finished", 1),
    ("memory note", 1),
];

/// A report to paste into a bug report. It holds no chat, card or note text,
/// and the user's folder is written as %USERPROFILE%.
#[tauri::command]
pub fn diagnostic_report(info: State<'_, StartInfo>) -> String {
    let mut out = vec![
        format!("Unstrung {}", env!("CARGO_PKG_VERSION")),
        format!(
            "System: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!(
            "Start: {}",
            match info.safe {
                None => "normal".to_owned(),
                Some(reason) => format!("safe start ({reason:?})"),
            }
        ),
        format!("Failed starts in a row: {}", start::attempts(&info.paths)),
    ];
    if !info.error.is_empty() {
        out.push(format!("Start error: {}", info.error));
    }
    out.push(format!("App folder: {}", info.paths.root().display()));
    out.push(String::new());
    out.push(format!("Last {LOG_LINES} lines of app.log:"));
    match std::fs::read_to_string(info.paths.logs().join("app.log")) {
        Ok(log) => {
            let lines: Vec<&str> = log.lines().collect();
            let from = lines.len().saturating_sub(LOG_LINES);
            out.extend(lines[from..].iter().map(|l| redact(l)));
        }
        Err(e) => out.push(format!("(no log: {e})")),
    }
    mask_home(&out.join("\n"))
}

/// Cuts the part of a log line that can hold chat text.
fn redact(line: &str) -> String {
    let mut line = line.to_owned();
    for (marker, keep) in CUT {
        if let Some(at) = line.find(marker) {
            let kept: Vec<&str> = line[at..].splitn(keep + 1, ": ").take(*keep).collect();
            let cut = kept.join(": ").len() < line.len() - at;
            line = format!(
                "{}{}{}",
                &line[..at],
                kept.join(": "),
                if cut { ": [removed]" } else { "" }
            );
        }
    }
    line.chars().take(240).collect()
}

fn mask_home(text: &str) -> String {
    let Ok(home) = std::env::var("USERPROFILE") else {
        return text.to_owned();
    };
    if home.is_empty() {
        return text.to_owned();
    }
    text.replace(&home, "%USERPROFILE%")
        .replace(&home.replace('\\', "/"), "%USERPROFILE%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_cuts_chat_text() {
        assert_eq!(
            redact("1 WARN unstrung_app::commands: command failed: invalid input: Mira said hi"),
            "1 WARN unstrung_app::commands: command failed: invalid input: [removed]"
        );
        assert_eq!(
            redact("1 INFO unstrung_core::chat: open thread in 812 ms: Some(\"the watch\")"),
            "1 INFO unstrung_core::chat: open thread in 812 ms: [removed]"
        );
        assert_eq!(
            redact("1 WARN unstrung_app::commands: command failed: the chat engine is not ready"),
            "1 WARN unstrung_app::commands: command failed: the chat engine is not ready"
        );
        assert_eq!(
            redact("1 INFO unstrung_core::chat: prompt tokens: estimate 894, engine 772"),
            "1 INFO unstrung_core::chat: prompt tokens: estimate 894, engine 772"
        );
    }
}
