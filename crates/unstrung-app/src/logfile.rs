//! A small file logger for `logs/app.log`. Chat text is never logged.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

/// The open log and its size. The app can stay open for weeks, so the log
/// rotates while it runs, not only at the start.
struct Sink {
    path: PathBuf,
    file: File,
    bytes: u64,
}

impl Sink {
    fn write(&mut self, line: &str) {
        if self.bytes > ROTATE_BYTES {
            // Windows renames a file that is open here, because Rust opens it
            // with delete sharing. If it fails, the next try is 5 MB later.
            self.bytes = 0;
            if std::fs::rename(&self.path, self.path.with_extension("log.1")).is_ok()
                && let Ok(file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)
            {
                self.file = file;
            }
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.bytes += line.len() as u64;
        }
    }
}

struct FileLogger(Mutex<Sink>);

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // The updater logs a failed check as an error, which it is not while
        // no release exists. updates.rs logs each outcome itself.
        metadata.level() <= log::Level::Info
            && !metadata.target().starts_with("tauri_plugin_updater")
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = line(record.level(), record.target(), record.args());
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Ok(mut sink) = self.0.lock() {
            sink.write(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.0.lock() {
            let _ = sink.file.flush();
        }
    }
}

fn line(level: log::Level, target: &str, message: impl std::fmt::Display) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("{secs} {level} {target}: {message}\n")
}

/// Adds one line for a process that must not start the logger, because
/// another process owns the log (a second start). Never rotates the file.
pub fn append_one(path: &Path, level: log::Level, target: &str, message: &str) {
    if let Ok(mut file) = OpenOptions::new().append(true).open(path) {
        let _ = file.write_all(line(level, target, message).as_bytes());
    }
}

pub fn init(path: &Path) -> std::io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    // An old log over the limit rotates at the first line.
    let bytes = file.metadata().map_or(0, |m| m.len());
    let sink = Sink {
        path: path.to_path_buf(),
        file,
        bytes,
    };
    if log::set_boxed_logger(Box::new(FileLogger(Mutex::new(sink)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    log_panics();
    Ok(())
}

/// Writes every panic to the log before the default hook runs. The app has
/// no console, so without this a panic that ends the process (for example,
/// one inside a Windows callback, which aborts) leaves no trace.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        log::error!(
            "panic in thread {thread} at {place}: {what}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        log::logger().flush();
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_rotates_while_the_app_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let mut sink = Sink {
            path: path.clone(),
            file,
            bytes: ROTATE_BYTES + 1,
        };
        sink.write(
            "new
",
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "new
"
        );
        assert!(path.with_extension("log.1").exists());
        assert_eq!(sink.bytes, 4);
    }
}
