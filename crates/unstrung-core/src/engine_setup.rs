//! Download and install of the chat engine (Ollama) from `catalog/engines.json`.
//! The engine goes into `engines/ollama`. A new version is unpacked next to it
//! and checked file by file before it replaces the old folder, so a failed
//! install never leaves a broken engine.

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

#[derive(Debug, Deserialize)]
struct Catalog {
    ollama: Release,
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
    /// "amd" for the add-on that only AMD graphics cards need.
    #[serde(default)]
    pub gpu: Option<String>,
    pub file: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub unpacked_size: u64,
}

pub fn release() -> &'static Release {
    static RELEASE: OnceLock<Release> = OnceLock::new();
    RELEASE.get_or_init(|| match serde_json::from_str::<Catalog>(ENGINES_JSON) {
        Ok(catalog) => catalog.ollama,
        Err(e) => {
            // A test checks the file, so this happens only with a broken build.
            log::error!("the engine catalog is not valid: {e}");
            Release::default()
        }
    })
}

/// Package id, then the path inside the package, then its SHA-256.
fn file_hashes() -> &'static BTreeMap<String, BTreeMap<String, String>> {
    static FILES: OnceLock<BTreeMap<String, BTreeMap<String, String>>> = OnceLock::new();
    FILES.get_or_init(|| {
        serde_json::from_str(ENGINE_FILES_JSON).unwrap_or_else(|e| {
            log::error!("the engine file list is not valid: {e}");
            BTreeMap::new()
        })
    })
}

fn is_amd(gpu: Option<&Gpu>) -> bool {
    gpu.is_some_and(|g| g.vendor == "AMD")
}

/// The packages that this PC needs: the base, plus the AMD add-on on AMD cards.
pub fn packages_for(gpu: Option<&Gpu>) -> Vec<&'static Package> {
    release()
        .packages
        .iter()
        .filter(|p| match p.gpu.as_deref() {
            None => true,
            Some("amd") => is_amd(gpu),
            Some(_) => false,
        })
        .collect()
}

/// Bytes to download.
pub fn download_bytes(gpu: Option<&Gpu>) -> u64 {
    packages_for(gpu).iter().map(|p| p.size).sum()
}

/// Free space needed at the peak: the zips and the unpacked files together.
pub fn disk_need(gpu: Option<&Gpu>) -> u64 {
    packages_for(gpu)
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

fn folder(paths: &AppPaths) -> PathBuf {
    paths.engines().join("ollama")
}

/// The version of the engine that Unstrung installed, if any.
pub fn installed_version(paths: &AppPaths) -> Option<String> {
    let bytes = std::fs::read(folder(paths).join(STAMP)).ok()?;
    let stamp: Stamp = serde_json::from_slice(&bytes).ok()?;
    folder(paths)
        .join("ollama.exe")
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
    progress: watch::Sender<Option<Progress>>,
    running: Mutex<Option<CancellationToken>>,
}

impl EngineSetup {
    pub fn new(paths: AppPaths) -> Arc<Self> {
        Arc::new(Self {
            paths,
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

    /// Downloads, unpacks and checks the engine into `engines/ollama.new`.
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
        let packages = packages_for(gpu);
        if packages.is_empty() {
            return Err(Error::Engine("the engine catalog is not valid".into()));
        }
        let downloads = self.paths.engines().join(".download");
        std::fs::create_dir_all(&downloads)?;
        // Space still needed: the zips that are not here yet, and the unpacked files.
        let have: u64 = packages
            .iter()
            .map(|p| {
                download::file_len(&download::part_name(&downloads.join(&p.file)))
                    .max(download::file_len(&downloads.join(&p.file)))
            })
            .sum();
        let need = disk_need(gpu).saturating_sub(have);
        let free = crate::hardware::disk_of(&downloads).map_or(0, |d| d.available);
        if free < need {
            return Err(Error::NoSpace { need: need - free });
        }

        let fetcher = Fetcher::new(&release().hosts)?;
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

        let new = self.paths.engines().join("ollama.new");
        let paths = self.paths.clone();
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
                let zip = paths.engines().join(".download").join(&package.file);
                let base = unpacked;
                unpacked += unpack(&zip, &new, cap, &token, &mut |n| {
                    this.set(Step::Unpacking, base + n, unpacked_total, 0, String::new());
                })?;
            }
            check_files(&new, &ids, &token, &mut |done, all| {
                this.set(Step::Checking, done, all, 0, String::new());
            })?;
            let stamp = Stamp {
                version: release().version.clone(),
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
        let current = folder(&self.paths);
        let new = self.paths.engines().join("ollama.new");
        let old = self.paths.engines().join("ollama.old");
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
        let _ = std::fs::remove_dir_all(self.paths.engines().join(".download"));
        let version = &release().version;
        self.set(Step::Done, 1, 1, 0, String::new());
        log::info!("chat engine {version} installed");
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
fn expected_files(ids: &[String]) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for id in ids {
        let list = file_hashes()
            .get(id)
            .ok_or_else(|| Error::Engine(format!("no file list for engine package {id}")))?;
        files.extend(list.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Ok(files)
}

/// Compares every file with the pinned list. Catches a bad unpack, which a
/// list made on this PC could not.
fn check_files(
    dir: &Path,
    ids: &[String],
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let bad = bad_files(dir, ids, cancel, progress)?;
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
    ids: &[String],
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<String>> {
    let files = expected_files(ids)?;
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

/// Full repair: the engine files that are missing or changed. Empty when the
/// engine is fine. Fails when Unstrung did not install the engine.
pub fn verify(paths: &AppPaths, cancel: &CancellationToken) -> Result<Vec<String>> {
    let bytes = std::fs::read(folder(paths).join(STAMP))?;
    let stamp: Stamp = serde_json::from_slice(&bytes)?;
    if stamp.version != release().version {
        return Ok(vec![format!(
            "version {} (now {})",
            stamp.version,
            release().version
        )]);
    }
    bad_files(&folder(paths), &stamp.packages, cancel, &mut |_, _| {})
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
        let r = release();
        assert!(!r.version.is_empty());
        assert!(r.packages.iter().any(|p| p.gpu.is_none()));
        for p in &r.packages {
            assert!(p.url.starts_with("https://"), "{}", p.url);
            assert_eq!(p.sha256.len(), 64);
            let host = p.url.split('/').nth(2).unwrap();
            assert!(r.hosts.iter().any(|h| h == host), "{host}");
            let files = file_hashes()
                .get(&p.id)
                .expect("a file list for each package");
            assert!(!files.is_empty());
        }
        let base = expected_files(&["base".into()]).unwrap();
        assert!(base.contains_key("ollama.exe"));
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
        assert_eq!(packages_for(Some(&nvidia)).len(), 1);
        assert_eq!(packages_for(None).len(), 1);
        assert_eq!(packages_for(Some(&amd)).len(), 2);
        assert!(download_bytes(Some(&amd)) > download_bytes(Some(&nvidia)));
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
    /// zip in `UNSTRUNG_TEST_ENGINE_ZIPS` (see `tools/engine_files.py`):
    /// `cargo test -p unstrung-core installs_the_pinned_engine -- --ignored`
    #[tokio::test]
    #[ignore = "needs the 1.6 GB engine zip"]
    async fn installs_the_pinned_engine() {
        let zips = PathBuf::from(std::env::var_os("UNSTRUNG_TEST_ENGINE_ZIPS").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let downloads = paths.engines().join(".download");
        std::fs::create_dir_all(&downloads).unwrap();
        let base = &packages_for(None)[0].file;
        std::fs::hard_link(zips.join(base), downloads.join(base)).unwrap();
        let setup = EngineSetup::new(paths.clone());
        setup.prepare(None).await.unwrap();
        setup.replace().unwrap();
        assert_eq!(installed_version(&paths), Some(release().version.clone()));
        assert!(folder(&paths).join("ollama.exe").is_file());
        assert!(
            verify(&paths, &CancellationToken::new())
                .unwrap()
                .is_empty()
        );
        assert!(!downloads.exists());
        // A changed file is found.
        std::fs::write(folder(&paths).join("ollama.exe"), b"changed").unwrap();
        assert_eq!(
            verify(&paths, &CancellationToken::new()).unwrap(),
            vec!["ollama.exe".to_owned()]
        );
    }

    #[test]
    fn replace_keeps_the_old_engine_when_the_new_one_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        std::fs::write(folder(&paths).join("ollama.exe"), b"old").unwrap();
        std::fs::create_dir_all(paths.engines().join("ollama.new")).unwrap();
        let setup = EngineSetup::new(paths.clone());
        assert!(setup.replace().is_err());
        assert_eq!(
            std::fs::read(folder(&paths).join("ollama.exe")).unwrap(),
            b"old"
        );

        let new = paths.engines().join("ollama.new");
        std::fs::write(new.join("ollama.exe"), b"new").unwrap();
        let stamp = Stamp {
            version: release().version.clone(),
            packages: vec!["base".into()],
        };
        std::fs::write(new.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
        setup.replace().unwrap();
        assert_eq!(
            std::fs::read(folder(&paths).join("ollama.exe")).unwrap(),
            b"new"
        );
        assert!(!new.exists() && !paths.engines().join("ollama.old").exists());
        assert_eq!(installed_version(&paths), Some(release().version.clone()));
    }
}
