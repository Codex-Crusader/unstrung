//! Runs `sd-cli.exe` for one picture. The process opens no network port, so
//! nothing outside the app can reach it, and when it exits all graphics
//! memory is free again. It runs in a kill-on-close job object, so it never
//! outlives the app.

use super::catalog::{self, ImageModel};
use super::scene::Prompts;
use crate::error::{Error, Result};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// What one picture needs.
pub struct Job<'a> {
    pub model: &'a ImageModel,
    /// `<models folder>/image`.
    pub models_dir: &'a Path,
    pub prompts: &'a Prompts,
    pub width: u32,
    pub height: u32,
    pub seed: i64,
    /// The PNG to write. It exists only after a good run.
    pub output: &'a Path,
}

/// The options for `sd-cli`.
pub fn args(job: &Job) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    let mut push = |k: &str, v: OsString| {
        a.push(k.into());
        a.push(v);
    };
    for (slot, c) in job.model.components() {
        let flag = format!("--{}", slot.replace('_', "-"));
        push(&flag, catalog::path_of(job.models_dir, c).into_os_string());
    }
    push("--prompt", job.prompts.positive.clone().into());
    push("--negative-prompt", job.prompts.negative.clone().into());
    push("--cfg-scale", job.model.cfg_scale.to_string().into());
    push("--steps", job.model.steps.to_string().into());
    push("--sampling-method", job.model.sampler.clone().into());
    push("--width", job.width.to_string().into());
    push("--height", job.height.to_string().into());
    push("--seed", job.seed.to_string().into());
    push("--output", job.output.as_os_str().to_owned());
    // Weights wait in RAM and move to the graphics card as needed, so a
    // model larger than the free VRAM still runs. The VAE works in tiles
    // for the same reason.
    a.push("--offload-to-cpu".into());
    a.push("--vae-tiling".into());
    // The prompt stays in the database, not inside an exported picture.
    a.push("--disable-image-metadata".into());
    a
}

/// A sampling progress line: "  |=====>     | 3/9 - 1.25s/it". The model
/// load prints other bars, without "s/it" or "it/s".
pub fn parse_progress(line: &str) -> Option<(u32, u32)> {
    let after_bar = line.rsplit('|').next()?;
    let (counts, speed) = after_bar.split_once(" - ")?;
    if !(speed.contains("s/it") || speed.contains("it/s")) {
        return None;
    }
    let (step, total) = counts.trim().split_once('/')?;
    let step = step.trim().parse().ok()?;
    let total = total.trim().parse().ok()?;
    (total > 0 && step <= total).then_some((step, total))
}

/// A random seed, kept in the database so "Regenerate (same seed)" works.
pub fn new_seed() -> i64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    i64::from(u32::try_from(h.finish() >> 33).unwrap_or(0))
}

/// Runs `cmd` until it exits. Its stdout and stderr go to `log`, and each
/// sampling step goes to `progress`. A cancel or the time limit kills it.
/// Fails unless the process exits with 0 and `output` exists.
pub fn run(
    mut cmd: Command,
    output: &Path,
    log: &Path,
    limit: Duration,
    cancel: &CancellationToken,
    progress: &(dyn Fn(u32, u32) + Sync),
) -> Result<()> {
    let _ = std::fs::remove_file(output);
    let mut log_file = crate::sidecar::open_log(log)?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(log_file.try_clone()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Engine(format!("the image engine did not start: {e}")))?;
    #[cfg(windows)]
    let _job = crate::sidecar::kill_on_close_job(&child);
    let mut stdout = child.stdout.take();

    let started = Instant::now();
    let result = std::thread::scope(|scope| {
        let reader = scope.spawn(move || {
            let Some(out) = stdout.as_mut() else { return };
            let mut buf = [0u8; 4096];
            let mut line = Vec::new();
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let _ = log_file.write_all(&buf[..n]);
                for &b in &buf[..n] {
                    if b == b'\r' || b == b'\n' {
                        if let Some((step, total)) = parse_progress(&String::from_utf8_lossy(&line))
                        {
                            progress(step, total);
                        }
                        line.clear();
                    } else {
                        line.push(b);
                    }
                }
            }
        });
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(e) => break Err(Error::Engine(format!("the image engine: {e}"))),
            }
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Error::Cancelled);
            }
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Error::Engine(format!(
                    "the picture took longer than {} minutes",
                    limit.as_secs() / 60
                )));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = reader.join();
        outcome
    });
    match result {
        Ok(status) if status.success() && output.is_file() => Ok(()),
        Ok(status) => {
            let _ = std::fs::remove_file(output);
            Err(Error::Engine(format!(
                "the image engine stopped ({status}). The log has the details."
            )))
        }
        Err(e) => {
            let _ = std::fs::remove_file(output);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sampling_steps_only() {
        assert_eq!(
            parse_progress("  |=====>    | 3/9 - 1.25s/it"),
            Some((3, 9))
        );
        assert_eq!(
            parse_progress("  |==========| 9/9 - 2.10it/s"),
            Some((9, 9))
        );
        assert_eq!(
            parse_progress("  |==>  | 12/20 -    10.5s/it - ETA 01:24"),
            Some((12, 20))
        );
        assert_eq!(parse_progress("  |====>  | 120/400 - 512.00MB/s"), None);
        assert_eq!(parse_progress("loading tensors"), None);
        assert_eq!(parse_progress("  |==| 10/9 - 1s/it"), None);
    }

    #[test]
    fn the_options_name_every_part() {
        let model = catalog::model("z-image-turbo").unwrap();
        let prompts = Prompts {
            positive: "a tavern".into(),
            negative: "blurry".into(),
        };
        let dir = Path::new("C:\\models\\image");
        let job = Job {
            model,
            models_dir: dir,
            prompts: &prompts,
            width: 1024,
            height: 1024,
            seed: 7,
            output: Path::new("C:\\out.png"),
        };
        let a: Vec<String> = args(&job)
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let value = |flag: &str| {
            let i = a
                .iter()
                .position(|x| x == flag)
                .unwrap_or_else(|| panic!("{flag}"));
            a[i + 1].clone()
        };
        assert!(value("--diffusion-model").ends_with("z_image_turbo-Q4_K.gguf"));
        assert!(value("--llm").ends_with("Qwen3-4B-Q4_K_M.gguf"));
        assert!(value("--vae").ends_with("flux1-ae.safetensors"));
        assert_eq!(value("--steps"), "9");
        assert_eq!(value("--seed"), "7");
        assert_eq!(value("--prompt"), "a tavern");
        assert!(a.contains(&"--disable-image-metadata".to_owned()));
    }

    #[test]
    fn seeds_are_positive_and_vary() {
        let a = new_seed();
        let b = new_seed();
        assert!(a >= 0 && b >= 0);
        assert_ne!(a, b);
    }

    /// One real picture on this PC's graphics card. Installs the image engine
    /// from the zips in `UNSTRUNG_TEST_ENGINE_ZIPS` and downloads Z-Image
    /// Turbo (6.70 GB) into `UNSTRUNG_TEST_HOME`, kept for the next run:
    /// `cargo test -p unstrung-core paints_one_real_picture -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "downloads 6.70 GB and runs the real engine"]
    async fn paints_one_real_picture() {
        use crate::download::{Fetcher, Source};
        use crate::engine_setup::{self as setup, Engine, EngineSetup};
        let home = std::path::PathBuf::from(std::env::var_os("UNSTRUNG_TEST_HOME").unwrap());
        let zips = std::path::PathBuf::from(std::env::var_os("UNSTRUNG_TEST_ENGINE_ZIPS").unwrap());
        let paths = crate::paths::AppPaths::at(home);
        paths.ensure().unwrap();
        let hw = crate::hardware::Hardware::check(&paths.models());
        let gpu = (hw.tier != crate::hardware::Tier::Basic)
            .then(|| hw.gpu.clone())
            .flatten();
        println!(
            "tier {:?}, gpu {:?}",
            hw.tier,
            gpu.as_ref().map(|g| &g.name)
        );
        if setup::installed_version(&paths, Engine::Image).as_deref()
            != Some(setup::release(Engine::Image).version.as_str())
        {
            let downloads = Engine::Image.downloads(&paths);
            std::fs::create_dir_all(&downloads).unwrap();
            for p in setup::packages_for(Engine::Image, gpu.as_ref()) {
                std::fs::copy(zips.join(&p.file), downloads.join(&p.file)).unwrap();
            }
            let s = EngineSetup::new(paths.clone(), Engine::Image);
            s.prepare(gpu.as_ref()).await.unwrap();
            s.replace().unwrap();
        }
        // UNSTRUNG_TEST_PICTURE_MODEL picks another catalog model, for example flux2-klein-4b.
        let id =
            std::env::var("UNSTRUNG_TEST_PICTURE_MODEL").unwrap_or_else(|_| "z-image-turbo".into());
        let model = catalog::model(&id).unwrap();
        let dir = paths.image_models();
        std::fs::create_dir_all(&dir).unwrap();
        let fetcher = Fetcher::new(&catalog::catalog().hosts).unwrap();
        for (_, c) in model.components() {
            if catalog::on_disk(&dir, c) {
                continue;
            }
            println!("downloading {} ({} bytes)", c.file, c.size);
            let source = Source {
                url: c.url.clone(),
                size: c.size,
                sha256: c.sha256.clone(),
            };
            let mut last = 0;
            let mut progress = |done: u64| {
                if done - last > 500_000_000 {
                    last = done;
                    println!("  {} MB", done / 1_000_000);
                }
            };
            fetcher
                .fetch(
                    &source,
                    &catalog::path_of(&dir, c),
                    &CancellationToken::new(),
                    &mut progress,
                )
                .await
                .unwrap();
        }

        let size = if gpu.is_some() { 1024 } else { 512 };
        let prompts = super::super::scene::Prompts {
            positive: "a cozy tavern at night, warm lantern light, wooden tables, a woman with red hair in a green cloak smiling, detailed illustration, fully clothed"
                .into(),
            negative: "blurry, low quality, nudity".into(),
        };
        let out = paths.gallery().join("test").join(format!("real-{id}.png"));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let job = Job {
            model,
            models_dir: &dir,
            prompts: &prompts,
            width: size,
            height: size,
            seed: 42,
            output: &out,
        };
        let exe = setup::folder(&paths, Engine::Image).join(Engine::Image.exe());
        let mut cmd = Command::new(exe);
        cmd.args(args(&job));
        // Peak graphics memory, read from nvidia-smi once a second.
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let watcher = std::thread::spawn(move || {
            let mut peak = 0u64;
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                if let Ok(o) = Command::new("nvidia-smi")
                    .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
                    .output()
                {
                    let used = String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse()
                        .unwrap_or(0);
                    peak = peak.max(used);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            peak
        });
        let started = Instant::now();
        let steps = std::sync::Mutex::new(Vec::new());
        let log = paths.logs().join("sdcpp.log");
        let result = run(
            cmd,
            &out,
            &log,
            Duration::from_secs(30 * 60),
            &CancellationToken::new(),
            &|s, t| {
                println!("  step {s}/{t} at {:.1} s", started.elapsed().as_secs_f32());
                steps.lock().unwrap().push((started.elapsed(), s));
            },
        );
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        let peak = watcher.join().unwrap();
        println!(
            "result {result:?}, {:.1} s, peak VRAM {peak} MiB",
            started.elapsed().as_secs_f32()
        );
        result.unwrap();
        let png = std::fs::read(&out).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let steps = steps.into_inner().unwrap();
        // The steps must come one by one, not all at the end.
        let first = steps.first().unwrap().0;
        let last = steps.last().unwrap().0;
        assert!(
            steps.len() >= 3 && last - first > Duration::from_millis(500),
            "{steps:?}"
        );
        println!("picture: {}", out.display());
    }

    /// A fake engine: PowerShell prints progress lines and writes the file.
    #[cfg(windows)]
    fn fake(script: &str) -> Command {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        cmd
    }

    #[cfg(windows)]
    #[test]
    fn a_good_run_reports_steps_and_keeps_the_picture() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("p.png");
        let log = dir.path().join("sdcpp.log");
        let script = format!(
            "[Console]::Out.Write(\"  |=>  | 1/2 - 1.00s/it`r\"); \
             [Console]::Out.Write(\"  |===| 2/2 - 1.00s/it`n\"); \
             Set-Content -LiteralPath '{}' -Value 'png'",
            out.display()
        );
        let seen = std::sync::Mutex::new(Vec::new());
        run(
            fake(&script),
            &out,
            &log,
            Duration::from_secs(60),
            &CancellationToken::new(),
            &|s, t| seen.lock().unwrap().push((s, t)),
        )
        .unwrap();
        assert!(out.is_file());
        assert_eq!(*seen.lock().unwrap(), [(1, 2), (2, 2)]);
        assert!(std::fs::read_to_string(&log).unwrap().contains("2/2"));
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_or_cancelled_run_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("p.png");
        let log = dir.path().join("sdcpp.log");
        // Writes the file, then fails: the half picture must go.
        let script = format!(
            "Set-Content -LiteralPath '{}' -Value 'x'; exit 3",
            out.display()
        );
        let err = run(
            fake(&script),
            &out,
            &log,
            Duration::from_secs(60),
            &CancellationToken::new(),
            &|_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("stopped"), "{err}");
        assert!(!out.exists());

        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = run(
            fake("Start-Sleep 30"),
            &out,
            &log,
            Duration::from_secs(60),
            &cancel,
            &|_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, Error::Cancelled));

        let started = Instant::now();
        let err = run(
            fake("Start-Sleep 30"),
            &out,
            &log,
            Duration::from_millis(500),
            &CancellationToken::new(),
            &|_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("longer than"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
