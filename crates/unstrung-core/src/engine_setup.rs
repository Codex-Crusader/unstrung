//! Download and install of the engines from `catalog/engines.json`: Ollama for
//! chat in `engines/ollama`, and stable-diffusion.cpp for pictures in
//! `engines/sdcpp`. A new version is unpacked next to the old folder and
//! checked file by file before it replaces it, so a failed install never
//! leaves a broken engine.

use crate::download::{self, Fetcher, Source};
use crate::error::{Error, Result};
use crate::files::write_atomic;
use crate::hardware::Gpu;
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const ENGINES_JSON: &str = include_str!("../catalog/engines.json");
/// SHA-256 of every file in each pinned package, made by `tools/engine_files.py`.
const ENGINE_FILES_JSON: &str = include_str!("../catalog/engine-files.json");
/// Written last, so a folder without it is not a finished install.
const STAMP: &str = "unstrung-engine.json";
/// Unpacked files may be a little larger than the catalog says, but not more.
const UNPACK_SLACK: f64 = 1.05;

/// The engines that Unstrung installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Ollama.
    Chat,
    /// stable-diffusion.cpp.
    Image,
}

impl Engine {
    /// The catalog key, the file list key and the folder name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Chat => "ollama",
            Self::Image => "sdcpp",
        }
    }

    /// The program that a finished install must have.
    pub fn exe(self) -> &'static str {
        match self {
            Self::Chat => "ollama.exe",
            Self::Image => "sd-cli.exe",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Chat => "chat engine",
            Self::Image => "image engine",
        }
    }

    /// Where the zips wait. The chat engine keeps its first folder name, so a
    /// download that an older version started goes on.
    pub fn downloads(self, paths: &AppPaths) -> PathBuf {
        match self {
            Self::Chat => paths.engines().join(".download"),
            Self::Image => paths.engines().join(".download-sdcpp"),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct Catalog {
    ollama: Release,
    sdcpp: Release,
}

#[derive(Debug, Default, Deserialize)]
pub struct Release {
    pub version: String,
    /// Download hosts, with the hosts that the URLs redirect to.
    pub hosts: Vec<String>,
    pub packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: String,
    /// "amd": an add-on that only AMD graphics cards get. "gpu": only with a
    /// usable graphics card. "none": only without one. Absent: always.
    #[serde(default)]
    pub gpu: Option<String>,
    pub file: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub unpacked_size: u64,
}

pub fn release(engine: Engine) -> &'static Release {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(ENGINES_JSON).unwrap_or_else(|e| {
            // A test checks the file, so this happens only with a broken build.
            log::error!("the engine catalog is not valid: {e}");
            Catalog::default()
        })
    });
    match engine {
        Engine::Chat => &catalog.ollama,
        Engine::Image => &catalog.sdcpp,
    }
}

/// Package id, then the path inside the package, then its SHA-256.
type FileLists = BTreeMap<String, BTreeMap<String, String>>;

fn file_hashes(engine: Engine) -> Option<&'static FileLists> {
    static FILES: OnceLock<BTreeMap<String, FileLists>> = OnceLock::new();
    FILES
        .get_or_init(|| {
            serde_json::from_str(ENGINE_FILES_JSON).unwrap_or_else(|e| {
                log::error!("the engine file list is not valid: {e}");
                BTreeMap::new()
            })
        })
        .get(engine.key())
}

fn is_amd(gpu: Option<&Gpu>) -> bool {
    gpu.is_some_and(|g| g.vendor == "AMD")
}

/// The packages that this PC needs. `gpu` is the usable graphics card, if any.
pub fn packages_for(engine: Engine, gpu: Option<&Gpu>) -> Vec<&'static Package> {
    release(engine)
        .packages
        .iter()
        .filter(|p| match p.gpu.as_deref() {
            None => true,
            Some("amd") => is_amd(gpu),
            Some("gpu") => gpu.is_some(),
            Some("none") => gpu.is_none(),
            Some(_) => false,
        })
        .collect()
}

/// Bytes to download.
pub fn download_bytes(engine: Engine, gpu: Option<&Gpu>) -> u64 {
    packages_for(engine, gpu).iter().map(|p| p.size).sum()
}

/// Free space needed at the peak: the zips and the unpacked files together.
pub fn disk_need(engine: Engine, gpu: Option<&Gpu>) -> u64 {
    packages_for(engine, gpu)
        .iter()
        .map(|p| p.size + p.unpacked_size)
        .sum::<u64>()
        + crate::location::MARGIN_BYTES
}

#[derive(Debug, Serialize, Deserialize)]
struct Stamp {
    version: String,
    packages: Vec<String>,
}

pub fn folder(paths: &AppPaths, engine: Engine) -> PathBuf {
    paths.engines().join(engine.key())
}

/// The version of the engine that Unstrung installed, if any.
pub fn installed_version(paths: &AppPaths, engine: Engine) -> Option<String> {
    let bytes = std::fs::read(folder(paths, engine).join(STAMP)).ok()?;
    let stamp: Stamp = serde_json::from_slice(&bytes).ok()?;
    folder(paths, engine)
        .join(engine.exe())
        .is_file()
        .then_some(stamp.version)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Downloading,
    Unpacking,
    Checking,
    Done,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub step: Step,
    pub completed: u64,
    pub total: u64,
    pub bytes_per_second: u64,
    /// Plain words, for a failed install.
    pub error: String,
}

pub struct EngineSetup {
    paths: AppPaths,
    engine: Engine,
    progress: watch::Sender<Option<Progress>>,
    running: Mutex<Option<CancellationToken>>,
}

impl EngineSetup {
    pub fn new(paths: AppPaths, engine: Engine) -> Arc<Self> {
        Arc::new(Self {
            paths,
            engine,
            progress: watch::channel(None).0,
            running: Mutex::new(None),
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<Progress>> {
        self.progress.subscribe()
    }

    pub fn current(&self) -> Option<Progress> {
        self.progress.borrow().clone()
    }

    pub fn cancel(&self) {
        if let Ok(running) = self.running.lock()
            && let Some(token) = running.as_ref()
        {
            token.cancel();
        }
    }

    fn set(&self, step: Step, completed: u64, total: u64, speed: u64, error: String) {
        self.progress.send_replace(Some(Progress {
            step,
            completed,
            total,
            bytes_per_second: speed,
            error,
        }));
    }

    /// Downloads, unpacks and checks the engine into `engines/<key>.new`.
    /// Then `replace` (called with the old engine stopped) swaps it in.
    /// Progress goes to `subscribe`. The new folder stays ready on success.
    pub async fn prepare(self: &Arc<Self>, gpu: Option<&Gpu>) -> Result<()> {
        let cancel = CancellationToken::new();
        {
            let mut running = self.running.lock().map_err(|_| Error::Busy)?;
            if running.is_some() {
                return Err(Error::Busy);
            }
            *running = Some(cancel.clone());
        }
        let result = self.prepare_inner(gpu, &cancel).await;
        if let Ok(mut running) = self.running.lock() {
            *running = None;
        }
        match &result {
            Ok(()) => {}
            Err(Error::Cancelled) => self.set(Step::Stopped, 0, 0, 0, String::new()),
            Err(e) => self.set(Step::Failed, 0, 0, 0, e.to_string()),
        }
        result
    }

    async fn prepare_inner(
        self: &Arc<Self>,
        gpu: Option<&Gpu>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let engine = self.engine;
        let packages = packages_for(engine, gpu);
        if packages.is_empty() {
            return Err(Error::Engine("the engine catalog is not valid".into()));
        }
        let downloads = engine.downloads(&self.paths);
        std::fs::create_dir_all(&downloads)?;
        // Space still needed: the zips that are not here yet, and the unpacked files.
        let have: u64 = packages
            .iter()
            .map(|p| {
                download::file_len(&download::part_name(&downloads.join(&p.file)))
                    .max(download::file_len(&downloads.join(&p.file)))
            })
            .sum();
        let need = disk_need(engine, gpu).saturating_sub(have);
        let free = crate::hardware::disk_of(&downloads).map_or(0, |d| d.available);
        if free < need {
            return Err(Error::NoSpace { need: need - free });
        }

        let fetcher = Fetcher::new(&release(engine).hosts)?;
        let total: u64 = packages.iter().map(|p| p.size).sum();
        let mut before = 0;
        let started = Instant::now();
        let mut first_byte: Option<u64> = None;
        for package in &packages {
            let source = Source {
                url: package.url.clone(),
                size: package.size,
                sha256: package.sha256.clone(),
            };
            let this = self.clone();
            let mut report = |done: u64| {
                let now = before + done;
                let start = *first_byte.get_or_insert(now);
                let secs = started.elapsed().as_secs().max(1);
                this.set(
                    Step::Downloading,
                    now,
                    total,
                    (now - start) / secs,
                    String::new(),
                );
            };
            fetcher
                .fetch(&source, &downloads.join(&package.file), cancel, &mut report)
                .await?;
            before += package.size;
        }

        let new = self.paths.engines().join(format!("{}.new", engine.key()));
        let this = self.clone();
        let ids: Vec<String> = packages.iter().map(|p| p.id.clone()).collect();
        let token = cancel.clone();
        tokio::task::spawn_blocking(move || {
            if new.exists() {
                std::fs::remove_dir_all(&new)?;
            }
            std::fs::create_dir_all(&new)?;
            let unpacked_total: u64 = packages.iter().map(|p| p.unpacked_size).sum();
            let mut unpacked = 0;
            for package in &packages {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let cap = (package.unpacked_size as f64 * UNPACK_SLACK) as u64;
                let zip = downloads.join(&package.file);
                let base = unpacked;
                unpacked += unpack(&zip, &new, cap, &token, &mut |n| {
                    this.set(Step::Unpacking, base + n, unpacked_total, 0, String::new());
                })?;
            }
            check_files(&new, engine, &ids, &token, &mut |done, all| {
                this.set(Step::Checking, done, all, 0, String::new());
            })?;
            let stamp = Stamp {
                version: release(engine).version.clone(),
                packages: ids,
            };
            write_atomic(&new.join(STAMP), &serde_json::to_vec_pretty(&stamp)?)?;
            Ok::<(), Error>(())
        })
        .await
        .map_err(|e| Error::Engine(format!("the engine install stopped: {e}")))??;
        Ok(())
    }

    /// Puts the prepared engine in place. The old engine must be stopped.
    /// Windows cannot rename a folder over another one, so the old folder
    /// moves aside first and comes back if the swap fails.
    pub fn replace(&self) -> Result<()> {
        let key = self.engine.key();
        let current = folder(&self.paths, self.engine);
        let new = self.paths.engines().join(format!("{key}.new"));
        let old = self.paths.engines().join(format!("{key}.old"));
        if !new.join(STAMP).is_file() {
            return Err(Error::Engine("the new engine is not ready".into()));
        }
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        if current.exists() {
            std::fs::rename(&current, &old)?;
        }
        if let Err(e) = std::fs::rename(&new, &current) {
            if old.exists() {
                let _ = std::fs::rename(&old, &current);
            }
            return Err(e.into());
        }
        if old.exists()
            && let Err(e) = std::fs::remove_dir_all(&old)
        {
            log::warn!("could not delete the old engine folder: {e}");
        }
        // The zips are not needed after a good install.
        let _ = std::fs::remove_dir_all(self.engine.downloads(&self.paths));
        let version = &release(self.engine).version;
        self.set(Step::Done, 1, 1, 0, String::new());
        log::info!("{} {version} installed", self.engine.label());
        Ok(())
    }
}

/// Unpacks a zip into `dest`. Every name must stay inside `dest`, and the
/// files together must stay under `cap` bytes. Returns the bytes written.
fn unpack(
    zip_path: &Path,
    dest: &Path,
    cap: u64,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .map_err(|e| Error::Engine(format!("the engine package cannot be read: {e}")))?;
    let mut written = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    for i in 0..archive.len() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::Engine(format!("the engine package is damaged: {e}")))?;
        let Some(name) = entry.enclosed_name() else {
            return Err(Error::Engine(format!(
                "the engine package has an unsafe file name: {}",
                String::from_utf8_lossy(entry.name_raw())
            )));
        };
        let path = dest.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&path)?;
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| Error::Engine(format!("the engine package is damaged: {e}")))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > cap {
                return Err(Error::Engine(
                    "the engine package is larger than the catalog says".into(),
                ));
            }
            out.write_all(&buf[..n])?;
            progress(written);
        }
    }
    Ok(written)
}

/// The files of the given packages, later packages over earlier ones, as
/// they lie in the unpacked folder.
fn expected_files(engine: Engine, ids: &[String]) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for id in ids {
        let list = file_hashes(engine)
            .and_then(|lists| lists.get(id))
            .ok_or_else(|| Error::Engine(format!("no file list for engine package {id}")))?;
        files.extend(list.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Ok(files)
}

/// Compares every file with the pinned list. Catches a bad unpack, which a
/// list made on this PC could not.
fn check_files(
    dir: &Path,
    engine: Engine,
    ids: &[String],
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let bad = bad_files(dir, engine, ids, cancel, progress)?;
    if bad.is_empty() {
        Ok(())
    } else {
        Err(Error::Engine(format!(
            "{} engine files do not match the catalog, for example {}",
            bad.len(),
            bad[0]
        )))
    }
}

fn bad_files(
    dir: &Path,
    engine: Engine,
    ids: &[String],
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<String>> {
    let files = expected_files(engine, ids)?;
    let all = files.len() as u64;
    let mut bad = Vec::new();
    for (i, (name, hash)) in files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let path = dir.join(name);
        match download::sha256_of(&path, &mut |_| {}) {
            Ok(h) if &h == hash => {}
            _ => bad.push(name.clone()),
        }
        progress(i as u64 + 1, all);
    }
    Ok(bad)
}

/// Repair: makes the app treat the engine as not installed, so the next
/// start offers "Install", which replaces the whole engine folder.
pub fn forget_install(paths: &AppPaths, engine: Engine) -> Result<()> {
    match std::fs::remove_file(folder(paths, engine).join(STAMP)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Full repair: the engine files that are missing or changed. Empty when the
/// engine is fine. Fails when Unstrung did not install the engine.
pub fn verify(paths: &AppPaths, engine: Engine, cancel: &CancellationToken) -> Result<Vec<String>> {
    let bytes = std::fs::read(folder(paths, engine).join(STAMP))?;
    let stamp: Stamp = serde_json::from_slice(&bytes)?;
    let now = &release(engine).version;
    if &stamp.version != now {
        return Ok(vec![format!("version {} (now {now})", stamp.version)]);
    }
    bad_files(
        &folder(paths, engine),
        engine,
        &stamp.packages,
        cancel,
        &mut |_, _| {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn the_catalog_and_its_file_lists_agree() {
        for engine in [Engine::Chat, Engine::Image] {
            let r = release(engine);
            assert!(!r.version.is_empty());
            for p in &r.packages {
                assert!(p.url.starts_with("https://"), "{}", p.url);
                assert_eq!(p.sha256.len(), 64);
                let host = p.url.split('/').nth(2).unwrap();
                assert!(r.hosts.iter().any(|h| h == host), "{host}");
                let files = expected_files(engine, std::slice::from_ref(&p.id)).unwrap();
                // An add-on goes over the base, so only full packages have the program.
                let add_on = p.gpu.as_deref() == Some("amd");
                assert!(
                    add_on || files.contains_key(engine.exe()),
                    "{} {}",
                    engine.key(),
                    p.id
                );
            }
            // Every PC gets an engine, with or without a graphics card.
            assert!(!packages_for(engine, None).is_empty());
        }
    }

    #[test]
    fn amd_cards_get_the_add_on() {
        let gpu = |vendor: &str| Gpu {
            name: "x".into(),
            vendor: vendor.into(),
            vram_bytes: 8 << 30,
            driver: String::new(),
        };
        let nvidia = gpu("NVIDIA");
        let amd = gpu("AMD");
        let chat = Engine::Chat;
        assert_eq!(packages_for(chat, Some(&nvidia)).len(), 1);
        assert_eq!(packages_for(chat, None).len(), 1);
        assert_eq!(packages_for(chat, Some(&amd)).len(), 2);
        assert!(download_bytes(chat, Some(&amd)) > download_bytes(chat, Some(&nvidia)));
    }

    #[test]
    fn the_image_engine_is_vulkan_with_a_card_and_cpu_without() {
        let gpu = Gpu {
            name: "x".into(),
            vendor: "AMD".into(),
            vram_bytes: 8 << 30,
            driver: String::new(),
        };
        let ids = |gpu| -> Vec<&str> {
            packages_for(Engine::Image, gpu)
                .iter()
                .map(|p| p.id.as_str())
                .collect()
        };
        assert_eq!(ids(Some(&gpu)), ["vulkan"]);
        assert_eq!(ids(None), ["cpu"]);
    }

    #[test]
    fn unpack_refuses_names_that_leave_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("bad.zip");
        make_zip(&zip, &[("ok.txt", b"ok"), ("../evil.txt", b"evil")]);
        let dest = dir.path().join("out");
        let err = unpack(&zip, &dest, 1000, &CancellationToken::new(), &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("unsafe file name"), "{err}");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn unpack_stops_at_the_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("big.zip");
        make_zip(&zip, &[("a.bin", &[0u8; 5000])]);
        let dest = dir.path().join("out");
        assert!(unpack(&zip, &dest, 4000, &CancellationToken::new(), &mut |_| {}).is_err());
        let written = unpack(&zip, &dest, 6000, &CancellationToken::new(), &mut |_| {}).unwrap();
        assert_eq!(written, 5000);
        assert_eq!(std::fs::read(dest.join("a.bin")).unwrap().len(), 5000);
    }

    /// The real pinned package, unpacked and checked file by file. Needs the
    /// zips in `UNSTRUNG_TEST_ENGINE_ZIPS` (see `tools/engine_files.py`):
    /// `cargo test -p unstrung-core installs_the_pinned -- --ignored`
    async fn installs_the_pinned(engine: Engine) {
        let zips = PathBuf::from(std::env::var_os("UNSTRUNG_TEST_ENGINE_ZIPS").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let downloads = engine.downloads(&paths);
        std::fs::create_dir_all(&downloads).unwrap();
        let file = &packages_for(engine, None)[0].file;
        std::fs::copy(zips.join(file), downloads.join(file)).unwrap();
        let setup = EngineSetup::new(paths.clone(), engine);
        setup.prepare(None).await.unwrap();
        setup.replace().unwrap();
        let version = Some(release(engine).version.clone());
        assert_eq!(installed_version(&paths, engine), version);
        let exe = folder(&paths, engine).join(engine.exe());
        assert!(exe.is_file());
        let cancel = CancellationToken::new();
        assert!(verify(&paths, engine, &cancel).unwrap().is_empty());
        assert!(!downloads.exists());
        // A changed file is found.
        std::fs::write(&exe, b"changed").unwrap();
        assert_eq!(
            verify(&paths, engine, &cancel).unwrap(),
            vec![engine.exe().to_owned()]
        );
    }

    #[tokio::test]
    #[ignore = "needs the 1.6 GB engine zip"]
    async fn installs_the_pinned_chat_engine() {
        installs_the_pinned(Engine::Chat).await;
    }

    #[tokio::test]
    #[ignore = "needs the 17 MB image engine zip"]
    async fn installs_the_pinned_image_engine() {
        installs_the_pinned(Engine::Image).await;
    }

    #[test]
    fn replace_keeps_the_old_engine_when_the_new_one_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let folder = |paths: &AppPaths| folder(paths, Engine::Chat);
        std::fs::write(folder(&paths).join("ollama.exe"), b"old").unwrap();
        std::fs::create_dir_all(paths.engines().join("ollama.new")).unwrap();
        let setup = EngineSetup::new(paths.clone(), Engine::Chat);
        assert!(setup.replace().is_err());
        assert_eq!(
            std::fs::read(folder(&paths).join("ollama.exe")).unwrap(),
            b"old"
        );

        let new = paths.engines().join("ollama.new");
        std::fs::write(new.join("ollama.exe"), b"new").unwrap();
        let stamp = Stamp {
            version: release(Engine::Chat).version.clone(),
            packages: vec!["base".into()],
        };
        std::fs::write(new.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
        setup.replace().unwrap();
        assert_eq!(
            std::fs::read(folder(&paths).join("ollama.exe")).unwrap(),
            b"new"
        );
        assert!(!new.exists() && !paths.engines().join("ollama.old").exists());
        assert_eq!(
            installed_version(&paths, Engine::Chat),
            Some(release(Engine::Chat).version.clone())
        );
    }
}
