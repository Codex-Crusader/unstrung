//! The model manager: the model list, downloads, removal, the default model
//! and the advanced settings.

use crate::catalog::{self, Advanced, StyleFollowing};
use crate::db::Db;
use crate::engine::{Engine, EngineStatus};
use crate::error::{Error, Result};
use crate::hardware::{Hardware, Tier};
use crate::ollama::{Client, Installed};
use crate::sidecar;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const META_MODEL: &str = "chat_model";
const META_ADVANCED: &str = "advanced_json";
const META_HARDWARE: &str = "hardware_fingerprint";

/// Extra free space to keep after a download, on top of the model size.
const DISK_MARGIN: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Downloading,
    /// A model file from disk: hashing it, then copying it into the engine.
    Importing,
    /// The engine checks the files after the last byte.
    Checking,
    Done,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Download {
    pub model: String,
    pub state: DownloadState,
    pub completed: u64,
    pub total: u64,
    pub bytes_per_second: u64,
    pub error: String,
}

/// One row of the Models screen.
#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub installed: bool,
    pub in_catalog: bool,
    pub recommended: bool,
    /// False when the model is too big for this PC's tier.
    pub fits: bool,
    pub min_tier: Option<Tier>,
    pub license_name: String,
    pub license_url: String,
    pub style_following: StyleFollowing,
    pub can_think: bool,
    pub is_default: bool,
    /// The installed files differ from the tested ones.
    pub changed_upstream: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelsPage {
    pub hardware: Hardware,
    pub models: Vec<ModelView>,
    pub default_model: String,
    pub advanced: Advanced,
    /// Debug builds share the models folder with the developer's own Ollama.
    /// Removal is off then.
    pub shared_folder: bool,
    pub download: Option<Download>,
    /// Set when the GPU or its driver changed since the last start.
    pub hardware_changed: bool,
}

struct Running {
    model: String,
    cancel: CancellationToken,
}

pub struct ModelService {
    engine: Arc<Engine>,
    db: Db,
    hardware: Mutex<Hardware>,
    hardware_changed: bool,
    running: Mutex<Option<Running>>,
    progress: watch::Sender<Option<Download>>,
}

/// What the app reads at start, before the engine runs.
pub struct Startup {
    pub model: String,
    pub advanced: Advanced,
    pub hardware: Hardware,
    pub hardware_changed: bool,
}

/// Runs the hardware check and reads the saved model settings.
pub async fn startup(db: &Db, engine_paths: &crate::paths::AppPaths) -> Result<Startup> {
    let (folder, _) = sidecar::models_folder(engine_paths);
    let check_at = if folder.is_dir() {
        folder
    } else {
        engine_paths.root().to_path_buf()
    };
    let hardware = tokio::task::spawn_blocking(move || Hardware::check(&check_at))
        .await
        .map_err(|e| Error::Engine(e.to_string()))?;
    let fingerprint = hardware.fingerprint();
    let before = db.meta(META_HARDWARE).await?;
    let hardware_changed = before.as_deref().is_some_and(|b| b != fingerprint);
    if before.as_deref() != Some(fingerprint.as_str()) {
        db.set_meta(META_HARDWARE, &fingerprint).await?;
    }
    if hardware_changed {
        log::info!(
            "the hardware changed: now {fingerprint}, tier {:?}",
            hardware.tier
        );
    }
    let model = match db.meta(META_MODEL).await? {
        Some(m) if !m.is_empty() => m,
        _ => catalog::recommended(hardware.tier).map_or_else(
            || crate::engine::DEFAULT_CHAT_MODEL.to_owned(),
            |e| e.id.clone(),
        ),
    };
    let advanced = db
        .meta(META_ADVANCED)
        .await?
        .and_then(|json| serde_json::from_str::<Advanced>(&json).ok())
        .filter(|a| a.check().is_ok())
        .unwrap_or_default();
    Ok(Startup {
        model,
        advanced,
        hardware,
        hardware_changed,
    })
}

impl ModelService {
    pub fn new(engine: Arc<Engine>, db: Db, startup: &Startup) -> Arc<Self> {
        Arc::new(Self {
            engine,
            db,
            hardware: Mutex::new(startup.hardware.clone()),
            hardware_changed: startup.hardware_changed,
            running: Mutex::new(None),
            progress: watch::channel(None).0,
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<Download>> {
        self.progress.subscribe()
    }

    /// After the engine starts: if the user never picked a model and the
    /// recommended one is not installed, use the best installed model.
    pub async fn after_start(&self) -> Result<()> {
        if !matches!(self.engine.status(), EngineStatus::NoModel { .. }) {
            return Ok(());
        }
        if self.db.meta(META_MODEL).await?.is_some() {
            return Ok(());
        }
        let installed = self.engine.refresh_installed().await?;
        let tier = self.tier();
        let pick = catalog::entries()
            .iter()
            .filter(|e| e.kind == "chat" && e.min_tier <= tier)
            .find(|e| installed.iter().any(|m| catalog::same_tag(&m.name, &e.id)))
            .map(|e| e.id.clone());
        if let Some(model) = pick {
            log::info!("the recommended model is not installed, so {model} is the default for now");
            self.engine.set_model(&model).await?;
        }
        Ok(())
    }

    fn tier(&self) -> Tier {
        self.hardware.lock().map_or(Tier::Basic, |h| h.tier)
    }

    fn shared_folder(&self) -> bool {
        sidecar::models_folder(self.engine.paths()).1
    }

    /// The result of the last hardware check.
    pub fn hardware(&self) -> Hardware {
        self.hardware
            .lock()
            .map_or_else(|e| e.into_inner().clone(), |h| h.clone())
    }

    /// Runs the hardware check again ("Check again" on the Models screen).
    pub async fn recheck_hardware(&self) -> Result<Hardware> {
        let startup = startup(&self.db, self.engine.paths()).await?;
        if let Ok(mut slot) = self.hardware.lock() {
            *slot = startup.hardware.clone();
        }
        Ok(startup.hardware)
    }

    pub async fn page(&self) -> Result<ModelsPage> {
        let installed = self.engine.refresh_installed().await.unwrap_or_default();
        let hardware = self
            .hardware
            .lock()
            .map_or_else(|_| Hardware::from_parts(None, 0, 0, false), |h| h.clone());
        let default_model = self.engine.model();
        Ok(ModelsPage {
            models: rows(&installed, hardware.tier, &default_model, |m| {
                self.engine.can_think(m)
            }),
            hardware,
            default_model,
            advanced: self.engine.advanced(),
            shared_folder: self.shared_folder(),
            download: self.progress.borrow().clone(),
            hardware_changed: self.hardware_changed,
        })
    }

    /// Starts a download in the background. Progress goes to `subscribe`.
    pub fn download(self: &Arc<Self>, model: &str) -> Result<()> {
        let runtime = runtime()?;
        let model = catalog::check_tag(model)?;
        let client = self.engine.running_client()?;
        let cancel = CancellationToken::new();
        {
            let mut running = self.running.lock().map_err(|_| Error::Busy)?;
            if let Some(r) = running.as_ref() {
                return Err(Error::Invalid(format!(
                    "{} is downloading now. Wait for it, or stop it first",
                    r.model
                )));
            }
            if let Some(entry) = catalog::entry(&model) {
                let free = self.hardware.lock().map_or(0, |h| h.disk_free_bytes);
                if free > 0 && free < entry.size_bytes + DISK_MARGIN {
                    return Err(Error::Invalid(format!(
                        "there is not enough free disk space: {} needs about {} GB",
                        entry.name,
                        (entry.size_bytes + DISK_MARGIN).div_ceil(1_000_000_000)
                    )));
                }
            }
            *running = Some(Running {
                model: model.clone(),
                cancel: cancel.clone(),
            });
        }
        let service = Arc::clone(self);
        runtime.spawn(async move {
            let expected = catalog::entry(&model).map_or(0, |e| e.size_bytes);
            let mut state = Progress::new(&model, expected);
            service.progress.send_replace(Some(state.view()));
            let result = client
                .pull(&model, &cancel, |line| {
                    state.add(line);
                    service.progress.send_replace(Some(state.view()));
                })
                .await;
            let (end, error) = match result {
                Ok(true) => (DownloadState::Done, String::new()),
                Ok(false) => (DownloadState::Stopped, String::new()),
                Err(e) => {
                    log::warn!("download of {model} failed: {e}");
                    (DownloadState::Failed, e.to_string())
                }
            };
            if let Ok(mut running) = service.running.lock() {
                *running = None;
            }
            service.engine.model_list_changed().await;
            if end == DownloadState::Done {
                service.after_download(&model).await;
            }
            let mut view = state.view();
            view.state = end;
            view.error = error;
            view.bytes_per_second = 0;
            service.progress.send_replace(Some(view));
        });
        Ok(())
    }

    /// Logs a model that differs from the tested one, and gives a model with no
    /// default one to use.
    async fn after_download(&self, model: &str) {
        let installed = self.engine.refresh_installed().await.unwrap_or_default();
        if let (Some(entry), Some(found)) = (
            catalog::entry(model),
            installed.iter().find(|m| catalog::same_tag(&m.name, model)),
        ) && !same_digest(&entry.digest, &found.digest)
        {
            log::warn!(
                "{model} changed upstream: tested {}, installed {}",
                entry.digest,
                found.digest
            );
        }
        if matches!(self.engine.status(), EngineStatus::NoModel { .. })
            && let Err(e) = self.set_default(model).await
        {
            log::warn!("could not use {model} as the default: {e}");
        }
    }

    /// Imports a GGUF model file from disk. It runs in the background like a
    /// download, in the same slot and with the same progress events. The
    /// engine copies the file into its models folder. Returns the model name.
    pub async fn import_file(self: &Arc<Self>, path: PathBuf) -> Result<String> {
        let client = self.engine.running_client()?;
        let size = check_gguf(&path)?;
        let taken: Vec<String> = client
            .installed()
            .await?
            .into_iter()
            .map(|m| m.name)
            .collect();
        let model = import_name(&path, &taken)?;
        let cancel = CancellationToken::new();
        {
            let mut running = self.running.lock().map_err(|_| Error::Busy)?;
            if let Some(r) = running.as_ref() {
                return Err(Error::Invalid(format!(
                    "{} is downloading now. Wait for it, or stop it first",
                    r.model
                )));
            }
            let free = self.hardware.lock().map_or(0, |h| h.disk_free_bytes);
            if free > 0 && free < size + DISK_MARGIN {
                return Err(Error::NoSpace {
                    need: size + DISK_MARGIN,
                });
            }
            *running = Some(Running {
                model: model.clone(),
                cancel: cancel.clone(),
            });
        }
        let service = Arc::clone(self);
        let name = model.clone();
        tokio::spawn(async move {
            let result = service
                .import_run(&client, &path, size, &name, &cancel)
                .await;
            let (end, error) = match result {
                Ok(()) => (DownloadState::Done, String::new()),
                Err(Error::Cancelled) => (DownloadState::Stopped, String::new()),
                Err(e) => {
                    log::warn!("import of {} failed: {e}", path.display());
                    (DownloadState::Failed, e.to_string())
                }
            };
            if let Ok(mut running) = service.running.lock() {
                *running = None;
            }
            service.engine.model_list_changed().await;
            if end == DownloadState::Done {
                log::info!("imported {} as {name}", path.display());
                service.after_download(&name).await;
            }
            service.progress.send_replace(Some(Download {
                model: name,
                state: end,
                completed: if end == DownloadState::Done {
                    2 * size
                } else {
                    0
                },
                total: 2 * size,
                bytes_per_second: 0,
                error,
            }));
        });
        Ok(model)
    }

    /// Hashes the file (the first half of the bar), then streams it to the
    /// engine (the second half), then makes the model.
    async fn import_run(
        self: &Arc<Self>,
        client: &Client,
        path: &Path,
        size: u64,
        model: &str,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let show = {
            let service = Arc::clone(self);
            let model = model.to_owned();
            move |completed: u64| {
                service.progress.send_replace(Some(Download {
                    model: model.clone(),
                    state: DownloadState::Importing,
                    completed,
                    total: 2 * size,
                    bytes_per_second: 0,
                    error: String::new(),
                }));
            }
        };
        show(0);
        let sha256 = {
            let path = path.to_owned();
            let show = show.clone();
            let cancel = cancel.clone();
            // Its own loop, not `sha256_of`, so Stop ends a long hash at once.
            tokio::task::spawn_blocking(move || -> Result<String> {
                use sha2::Digest;
                use std::io::Read;
                let mut file = std::fs::File::open(&path)?;
                let mut hasher = sha2::Sha256::new();
                let mut buf = vec![0u8; 1 << 20];
                let mut done = 0;
                loop {
                    if cancel.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    done += n as u64;
                    show(done);
                }
                Ok(crate::download::hex(&hasher.finalize()))
            })
            .await
            .map_err(|e| Error::Engine(e.to_string()))??
        };
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if !client.has_blob(&sha256).await? {
            let file = tokio::fs::File::open(path).await?;
            let sent = std::sync::atomic::AtomicU64::new(0);
            let stream = futures_util::stream::try_unfold(
                (file, show.clone(), sent),
                move |(mut file, show, sent)| async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = vec![0u8; 1 << 20];
                    let n = file.read(&mut buf).await?;
                    if n == 0 {
                        return Ok::<_, std::io::Error>(None);
                    }
                    buf.truncate(n);
                    let total =
                        sent.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed) + n as u64;
                    show(size + total);
                    Ok(Some((buf, (file, show, sent))))
                },
            );
            tokio::select! {
                result = client.push_blob(&sha256, reqwest::Body::wrap_stream(stream)) => result?,
                () = cancel.cancelled() => return Err(Error::Cancelled),
            }
        }
        show(2 * size);
        let file_name = path
            .file_name()
            .map_or_else(|| "model.gguf".into(), |n| n.to_string_lossy().into_owned());
        client.create_from_gguf(model, &file_name, &sha256).await
    }

    /// Stops the running download. The engine keeps the finished parts.
    pub fn stop_download(&self) {
        if let Ok(running) = self.running.lock()
            && let Some(r) = running.as_ref()
        {
            r.cancel.cancel();
        }
    }

    pub async fn remove(&self, model: &str) -> Result<()> {
        if self.shared_folder() {
            return Err(Error::Invalid(
                "this build shares its models with your own Ollama, so it does not remove them"
                    .into(),
            ));
        }
        if catalog::same_tag(model, &self.engine.model()) {
            return Err(Error::Invalid(
                "this is the default model. Pick another default first".into(),
            ));
        }
        let client = self.engine.running_client()?;
        let _ = client.unload(model).await;
        client.delete(model).await?;
        self.engine.model_list_changed().await;
        Ok(())
    }

    pub async fn set_default(&self, model: &str) -> Result<()> {
        self.engine.refresh_installed().await?;
        if !self.engine.is_installed(model) {
            return Err(Error::Invalid(format!("{model} is not installed")));
        }
        self.engine.set_model(model).await?;
        self.db.set_meta(META_MODEL, model).await
    }

    pub async fn set_advanced(&self, advanced: Advanced) -> Result<()> {
        advanced.check()?;
        self.db
            .set_meta(META_ADVANCED, &serde_json::to_string(&advanced)?)
            .await?;
        self.engine.set_advanced(advanced);
        Ok(())
    }
}

fn same_digest(a: &str, b: &str) -> bool {
    a.trim_start_matches("sha256:") == b.trim_start_matches("sha256:")
}

/// The catalog models first (in catalog order), then the other installed models.
fn rows(
    installed: &[Installed],
    tier: Tier,
    default_model: &str,
    can_think: impl Fn(&str) -> bool,
) -> Vec<ModelView> {
    let find = |id: &str| installed.iter().find(|m| catalog::same_tag(&m.name, id));
    let recommended = catalog::recommended(tier).map(|e| e.id.as_str());
    let mut out: Vec<ModelView> = catalog::entries()
        .iter()
        .filter(|e| e.kind == "chat")
        .map(|e| {
            let found = find(&e.id);
            ModelView {
                id: e.id.clone(),
                name: e.name.clone(),
                description: e.description.clone(),
                size_bytes: e.size_bytes,
                installed: found.is_some(),
                in_catalog: true,
                recommended: recommended == Some(e.id.as_str()),
                fits: e.min_tier <= tier,
                min_tier: Some(e.min_tier),
                license_name: e.license.name.clone(),
                license_url: e.license.url.clone(),
                style_following: e.tuning.style_following,
                can_think: e.tuning.think.is_some(),
                is_default: catalog::same_tag(&e.id, default_model),
                changed_upstream: found.is_some_and(|m| !same_digest(&m.digest, &e.digest)),
            }
        })
        .collect();
    for m in installed {
        if catalog::entry(&m.name).is_some() {
            continue;
        }
        out.push(ModelView {
            id: m.name.clone(),
            name: m.name.clone(),
            description: [
                m.details.parameter_size.as_str(),
                m.details.quantization_level.as_str(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
            size_bytes: m.size,
            installed: true,
            in_catalog: false,
            recommended: false,
            fits: true,
            min_tier: None,
            license_name: String::new(),
            license_url: String::new(),
            style_following: StyleFollowing::Fair,
            can_think: can_think(&m.name),
            is_default: catalog::same_tag(&m.name, default_model),
            changed_upstream: false,
        });
    }
    out
}

/// Turns the per-file progress lines of a download into one total and a speed.
struct Progress {
    model: String,
    /// The catalog size. The engine names the files one by one, so their sum
    /// is too small until the last file starts.
    expected: u64,
    files: HashMap<String, (u64, u64)>,
    state: DownloadState,
    /// (time, bytes done) samples for the speed, over the last few seconds.
    samples: Vec<(Instant, u64)>,
}

impl Progress {
    fn new(model: &str, expected: u64) -> Self {
        Self {
            model: model.to_owned(),
            expected,
            files: HashMap::new(),
            state: DownloadState::Downloading,
            samples: Vec::new(),
        }
    }

    fn add(&mut self, line: &crate::ollama::PullLine) {
        if !line.digest.is_empty() && line.total > 0 {
            self.files.insert(
                line.digest.clone(),
                (line.completed.min(line.total), line.total),
            );
        }
        if line.status.starts_with("verifying") || line.status.starts_with("writing") {
            self.state = DownloadState::Checking;
        }
        let now = Instant::now();
        let done = self.files.values().map(|f| f.0).sum();
        self.samples.push((now, done));
        self.samples
            .retain(|(t, _)| now.duration_since(*t).as_secs_f32() <= 4.0);
    }

    fn view(&self) -> Download {
        let completed = self.files.values().map(|f| f.0).sum();
        let total = self
            .files
            .values()
            .map(|f| f.1)
            .sum::<u64>()
            .max(self.expected);
        let bytes_per_second = match (self.samples.first(), self.samples.last()) {
            (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                let secs = t1.duration_since(*t0).as_secs_f64();
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let speed = (b1.saturating_sub(*b0) as f64 / secs) as u64;
                speed
            }
            _ => 0,
        };
        Download {
            model: self.model.clone(),
            state: self.state,
            completed,
            total,
            bytes_per_second,
            error: String::new(),
        }
    }
}

/// A GGUF file starts with these four bytes.
const GGUF_MAGIC: &[u8; 4] = b"GGUF";

/// Checks that `path` is a GGUF model file. Returns its size.
fn check_gguf(path: &Path) -> Result<u64> {
    let not_gguf = |why: String| Error::Rejected {
        message: "This is not a GGUF model file. Pick a file that ends in .gguf.".into(),
        why,
    };
    let gguf_name = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    if !gguf_name {
        return Err(not_gguf(format!("{} has no .gguf ending", path.display())));
    }
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(not_gguf(format!("{} is not a file", path.display())));
    }
    let mut magic = [0u8; 4];
    std::io::Read::read_exact(&mut std::fs::File::open(path)?, &mut magic)
        .map_err(|e| not_gguf(e.to_string()))?;
    if &magic != GGUF_MAGIC {
        return Err(not_gguf(format!(
            "the file starts with {magic:?}, not GGUF"
        )));
    }
    Ok(meta.len())
}

/// A model name from the file name: lowercase letters, digits and - . _,
/// with a number added when the engine already has that name.
fn import_name(path: &Path, taken: &[String]) -> Result<String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut base = String::new();
    for c in stem.chars() {
        let c = if c.is_ascii_alphanumeric() || "._".contains(c) {
            c
        } else {
            '-'
        };
        if !(c == '-' && base.ends_with('-')) {
            base.push(c);
        }
    }
    let mut base: String = base
        .trim_matches(|c| "-._".contains(c))
        .chars()
        .take(40)
        .collect();
    base = base.trim_end_matches(|c| "-._".contains(c)).to_owned();
    if base.is_empty() {
        base = "imported-model".into();
    }
    let free = |name: &str| !taken.iter().any(|t| catalog::same_tag(t, name));
    let name = (1..1000)
        .map(|n| {
            if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            }
        })
        .find(|name| free(name))
        .ok_or_else(|| Error::Invalid("pick a file with another name".into()))?;
    catalog::check_tag(&name)
}

/// The Tokio runtime of the caller. A caller on a thread with no runtime
/// (for example, a sync Tauri command on the main thread) gets an error
/// instead of a panic, which would end the app.
fn runtime() -> Result<tokio::runtime::Handle> {
    tokio::runtime::Handle::try_current()
        .map_err(|_| Error::Invalid("a download must start inside the async runtime".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::{InstalledDetails, PullLine};

    #[test]
    fn import_names_are_clean_and_free() {
        let taken = vec!["my-model:latest".to_owned(), "gemma4:e4b-it-qat".to_owned()];
        let name = |p: &str| super::import_name(Path::new(p), &taken).unwrap();
        assert_eq!(
            name("C:/m/Mistral 7B Instruct (Q4_K_M).gguf"),
            "mistral-7b-instruct-q4_k_m"
        );
        assert_eq!(name("C:/m/my-model.gguf"), "my-model-2");
        assert_eq!(name("C:/m/!!!.gguf"), "imported-model");
        assert_eq!(name("C:/m/über-modell.GGUF"), "ber-modell");
    }

    #[test]
    fn only_gguf_files_import() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("a.gguf");
        std::fs::write(&good, b"GGUF\x03\x00\x00\x00rest").unwrap();
        assert_eq!(super::check_gguf(&good).unwrap(), 12);
        let fake = dir.path().join("b.gguf");
        std::fs::write(&fake, b"PK\x03\x04 a zip").unwrap();
        assert!(matches!(
            super::check_gguf(&fake),
            Err(Error::Rejected { .. })
        ));
        let short = dir.path().join("c.gguf");
        std::fs::write(&short, b"GG").unwrap();
        assert!(super::check_gguf(&short).is_err());
        let named = dir.path().join("d.bin");
        std::fs::write(&named, b"GGUF").unwrap();
        assert!(super::check_gguf(&named).is_err());
    }

    #[test]
    fn a_download_outside_the_runtime_is_an_error() {
        assert!(super::runtime().is_err());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(rt.block_on(async { super::runtime() }).is_ok());
    }

    #[test]
    fn progress_adds_files() {
        let mut p = Progress::new("m", 0);
        let line = |digest: &str, completed, total| PullLine {
            status: format!("pulling {digest}"),
            digest: digest.into(),
            total,
            completed,
            ..PullLine::default()
        };
        p.add(&line("a", 50, 100));
        p.add(&line("b", 0, 10));
        p.add(&line("a", 100, 100));
        let v = p.view();
        assert_eq!(
            (v.completed, v.total, v.state),
            (100, 110, DownloadState::Downloading)
        );
        p.add(&PullLine {
            status: "verifying sha256 digest".into(),
            ..PullLine::default()
        });
        assert_eq!(p.view().state, DownloadState::Checking);
        // Before the big file starts, the total is the catalog size.
        let mut p = Progress::new("m", 1000);
        p.add(&line("a", 50, 100));
        assert_eq!((p.view().completed, p.view().total), (50, 1000));
    }

    #[test]
    fn rows_merge_catalog_and_installed() {
        let first = &catalog::entries()[0];
        let installed = vec![
            Installed {
                name: first.id.clone(),
                size: 1,
                digest: "other".into(),
                details: InstalledDetails::default(),
            },
            Installed {
                name: "someone/custom:q4".into(),
                size: 2,
                digest: String::new(),
                details: InstalledDetails {
                    parameter_size: "8B".into(),
                    ..InstalledDetails::default()
                },
            },
        ];
        let list = rows(&installed, Tier::Standard, "someone/custom:q4", |m| {
            m.starts_with("someone")
        });
        let row = list.iter().find(|r| r.id == first.id).unwrap();
        assert!(row.installed && row.in_catalog && row.changed_upstream && !row.is_default);
        let custom = list.last().unwrap();
        assert!(
            !custom.in_catalog
                && custom.is_default
                && custom.description == "8B"
                && custom.can_think
        );
        assert_eq!(list.len(), catalog::entries().len() + 1);
    }
}
