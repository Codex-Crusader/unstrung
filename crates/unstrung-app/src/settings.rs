//! Settings that only the window uses: theme, text size and motion. They
//! live in the `meta` table, each with a fixed list of values.

use crate::commands::CmdResult;
use crate::state::AppState;
use std::collections::BTreeMap;
use tauri::{Manager, State};
use unstrung_core::Error;

pub const THEME_KEY: &str = "ui_theme";

/// Each setting with its values. The first value is the default.
const CHOICES: [(&str, &[&str]); 3] = [
    (THEME_KEY, &["system", "light", "dark", "contrast"]),
    ("ui_text_size", &["normal", "large", "larger"]),
    ("ui_motion", &["system", "reduce"]),
];

/// The saved value, or the default for one that is missing or unknown.
pub fn value_of(key: &str, saved: Option<&str>) -> &'static str {
    let Some((_, values)) = CHOICES.iter().find(|(k, _)| *k == key) else {
        return "";
    };
    values
        .iter()
        .find(|v| Some(**v) == saved)
        .copied()
        .unwrap_or(values[0])
}

/// The window theme for a theme setting. `None` follows Windows.
pub fn window_theme(theme: &str) -> Option<tauri::Theme> {
    match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" | "contrast" => Some(tauri::Theme::Dark),
        _ => None,
    }
}

#[tauri::command]
pub async fn get_ui_settings(state: State<'_, AppState>) -> CmdResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (key, _) in CHOICES {
        let saved = state.chats.db().meta(key).await?;
        out.insert(key.to_owned(), value_of(key, saved.as_deref()).to_owned());
    }
    Ok(out)
}

#[tauri::command]
pub async fn set_ui_setting(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CmdResult<()> {
    let known = CHOICES
        .iter()
        .any(|(k, values)| *k == key && values.contains(&value.as_str()));
    if !known {
        return Err(Error::Invalid(format!("{key} cannot be {value}")).into());
    }
    state.chats.db().set_meta(&key, &value).await?;
    if key == THEME_KEY
        && let Some(window) = app.get_webview_window("main")
    {
        // Only the title bar and the page's color scheme; the page also
        // reads the setting itself, so a failure here changes little.
        if let Err(e) = window.set_theme(window_theme(&value)) {
            log::warn!("could not set the window theme: {e}");
        }
    }
    Ok(())
}

/// Restarts the app in safe start, where quick repair and the copies of the
/// chats are.
#[tauri::command]
pub fn restart_safe(app: tauri::AppHandle) -> CmdResult<()> {
    log::info!("restart in safe start, from Settings");
    crate::instance::relaunch(&app, true)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_values_fall_back_to_the_default() {
        assert_eq!(value_of(THEME_KEY, Some("dark")), "dark");
        assert_eq!(value_of(THEME_KEY, Some("purple")), "system");
        assert_eq!(value_of(THEME_KEY, None), "system");
        assert_eq!(value_of("ui_text_size", Some("larger")), "larger");
        assert_eq!(value_of("nothing", Some("x")), "");
    }
}
