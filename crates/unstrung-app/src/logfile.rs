//! A small file logger for `logs/app.log`. Chat text is never logged.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const ROTATE_BYTES: u64 = 5 * 1024 * 1024;

struct FileLogger(Mutex<File>);

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let line = format!(
            "{secs} {} {}: {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        if let Ok(mut file) = self.0.lock() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

pub fn init(path: &Path) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    if log::set_boxed_logger(Box::new(FileLogger(Mutex::new(file)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    Ok(())
}
