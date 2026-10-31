//! Fixed locations inside the app folder.
//!
//! Every path is built here from the root plus a fixed name or an ID.
//! Paths never come from the UI.

use crate::error::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Overrides the app folder. Tests and the self-test use it, so they never touch real data.
pub const HOME_ENV: &str = "UNSTRUNG_HOME";

/// Overrides the folder that the chat engine reads its models from. For development only.
pub const DEV_MODELS_ENV: &str = "UNSTRUNG_DEV_OLLAMA_MODELS";

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// `UNSTRUNG_HOME` if set, else `%LOCALAPPDATA%\Unstrung`.
    pub fn resolve() -> Result<Self> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::at(PathBuf::from(home)));
        }
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::Invalid("LOCALAPPDATA is not set".into()))?;
        Ok(Self::at(PathBuf::from(local).join("Unstrung")))
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Creates the folders that the app writes to.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            self.root.clone(),
            self.avatars(),
            self.backups(),
            self.logs(),
            self.engines().join("ollama"),
            self.ollama_models(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("app.db")
    }

    pub fn avatars(&self) -> PathBuf {
        self.root.join("avatars")
    }

    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }

    /// Held locked while an Unstrung process uses this folder (see `start::lock`).
    pub fn lock_file(&self) -> PathBuf {
        self.root.join("app.lock")
    }

    /// The count of starts in a row that did not load (see `start`).
    pub fn start_attempts(&self) -> PathBuf {
        self.root.join("start-attempts.txt")
    }

    /// Process IDs of the engines that this app started. Used to stop leftovers.
    pub fn engine_pids(&self) -> PathBuf {
        self.root.join("engine-pids.json")
    }

    /// The models folder of the private chat engine (`OLLAMA_MODELS`).
    /// Phase 1 keeps the library folder inside the app folder.
    pub fn ollama_models(&self) -> PathBuf {
        self.root.join("models").join("ollama")
    }

    /// Joins a path that the database stores (relative to the app folder).
    /// Rejects anything that could leave the root: `..`, absolute paths, drive prefixes.
    pub fn stored(&self, relative: &str) -> Result<PathBuf> {
        let rel = Path::new(relative);
        let plain = !relative.is_empty()
            && rel
                .components()
                .all(|c| matches!(c, Component::Normal(part) if is_safe_part(part)));
        if !plain {
            return Err(Error::Invalid(format!("unsafe stored path: {relative}")));
        }
        Ok(self.root.join(rel))
    }
}

/// Windows device names (`CON`, `NUL`, `COM1`) open a device, not a file.
fn is_safe_part(part: &std::ffi::OsStr) -> bool {
    let Some(text) = part.to_str() else {
        return false;
    };
    let stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    !device && !text.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_paths_stay_inside_the_root() {
        let paths = AppPaths::at(PathBuf::from("C:\\data"));
        assert!(paths.stored("avatars/ren.png").is_ok());
        for bad in [
            "",
            "..\\x",
            "avatars/../../x",
            "C:\\Windows\\x",
            "\\\\server\\share",
            "/etc/passwd",
            "avatars/CON",
            "avatars/nul.png",
            "COM1",
            "avatars/a:b",
        ] {
            assert!(paths.stored(bad).is_err(), "{bad} must be rejected");
        }
    }
}
