//! Downloads of large files from fixed catalog sources: the chat engine now,
//! image models in Phase 5. A file goes to `<name>.part`, resumes after a
//! break, and gets its final name only after its SHA-256 matches the catalog.

use crate::error::{Error, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// No bytes for this long counts as a break. The next try resumes.
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 5;

/// One file from the catalog.
#[derive(Debug, Clone)]
pub struct Source {
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// A client that only goes to the given hosts. Every redirect must stay on
/// them, so a download never ends up somewhere the catalog does not name.
pub struct Fetcher {
    client: reqwest::Client,
}

impl Fetcher {
    pub fn new(hosts: &[String]) -> Result<Self> {
        Self::build(hosts, true)
    }

    fn build(hosts: &[String], https_only: bool) -> Result<Self> {
        let allowed: Vec<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            let url = attempt.url().clone();
            let host_ok = url.host_str().is_some_and(|h| host_allowed(&allowed, h));
            let scheme_ok = url.scheme() == "https" || !https_only;
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if host_ok && scheme_ok {
                attempt.follow()
            } else {
                attempt.error(format!("redirect to a host that is not allowed: {url}"))
            }
        });
        let client = reqwest::Client::builder()
            .https_only(https_only)
            .redirect(policy)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("Unstrung/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(hosts: &[String]) -> Self {
        Self::build(hosts, false).unwrap()
    }

    /// Downloads `source` to `target`. `progress` gets the bytes on disk so
    /// far, also the bytes from an earlier try. Returns when `target` holds
    /// the checked file.
    pub async fn fetch(
        &self,
        source: &Source,
        target: &Path,
        cancel: &CancellationToken,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<()> {
        if file_len(target) == source.size && sha256_of(target, &mut |_| {})? == source.sha256 {
            return Ok(());
        }
        let part = part_name(target);
        let mut have = file_len(&part);
        if have > source.size {
            truncate(&part)?;
            have = 0;
        }
        if have < source.size {
            have = self.get(source, &part, have, cancel, progress).await?;
        }
        if have != source.size {
            return Err(Error::Engine(format!(
                "the download ended at {have} of {} bytes",
                source.size
            )));
        }
        let hash = sha256_of(&part, &mut |_| {})?;
        if hash != source.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Rejected {
                message: "The download was damaged, so it was deleted. Try again.".into(),
                why: format!("SHA-256 {hash}, expected {}", source.sha256),
            });
        }
        std::fs::rename(&part, target)?;
        Ok(())
    }

    /// One request from byte `have`. Always from the catalog URL: a redirect
    /// target is short-lived and is never saved.
    async fn get(
        &self,
        source: &Source,
        part: &Path,
        have: u64,
        cancel: &CancellationToken,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<u64> {
        let mut request = self.client.get(&source.url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let response = tokio::select! {
            r = request.send() => r.map_err(offline_or_http)?,
            () = cancel.cancelled() => return Err(Error::Cancelled),
        };
        let status = response.status();
        let mut start = have;
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let from = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(range_start);
            if from != Some(have) {
                return Err(Error::Engine(
                    "the server sent the wrong part of the file".into(),
                ));
            }
        } else if status.is_success() {
            // The server ignored the range: start again from the first byte.
            start = 0;
        } else {
            return Err(Error::Engine(format!(
                "the download server answered {status}"
            )));
        }
        if let Some(len) = response.content_length()
            && start + len != source.size
        {
            return Err(Error::Engine(format!(
                "the file is {} bytes, but the catalog says {}",
                start + len,
                source.size
            )));
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(part)?;
        file.set_len(start)?;
        file.seek(SeekFrom::Start(start))?;
        let mut done = start;
        progress(done);
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                () = cancel.cancelled() => {
                    file.sync_all()?;
                    return Err(Error::Cancelled);
                }
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(offline_or_http)?;
            done += chunk.len() as u64;
            if done > source.size {
                return Err(Error::Engine(
                    "the server sent more than the catalog size".into(),
                ));
            }
            file.write_all(&chunk)?;
            progress(done);
        }
        file.sync_all()?;
        Ok(done)
    }
}

/// A lost connection or a timeout is "no internet", which a retry resumes.
/// An entry that starts with a dot allows every host under it: ".hf.co"
/// allows "us.aws.cdn.hf.co", because download CDNs change the host by region.
fn host_allowed(allowed: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    allowed
        .iter()
        .any(|a| *a == host || (a.starts_with('.') && host.ends_with(a.as_str())))
}

fn offline_or_http(e: reqwest::Error) -> Error {
    if e.is_connect() || e.is_timeout() {
        Error::Offline(e.to_string())
    } else {
        Error::Http(e)
    }
}

/// The `.part` file next to `target`. Quick repair deletes old ones.
pub fn part_name(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    target.with_file_name(name)
}

/// `bytes 100-999/1000` gives 100.
fn range_start(value: &str) -> Option<u64> {
    value
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

pub(crate) fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

fn truncate(path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    Ok(())
}

/// The SHA-256 of a file as lowercase hex. `progress` gets the bytes read.
pub fn sha256_of(path: &Path, progress: &mut dyn FnMut(u64)) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut read = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read += n as u64;
        progress(read);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Clone, Copy)]
    pub(crate) enum Mode {
        /// Honors Range requests.
        Ranges,
        /// Always sends the whole file with 200.
        IgnoresRanges,
        /// Sends only the first half, then closes.
        Breaks,
        /// Redirects to the given port on 127.0.0.1.
        RedirectTo(u16),
    }

    /// A tiny HTTP server for one file. Returns its port.
    pub(crate) async fn serve(body: Vec<u8>, mode: Mode) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut req = vec![0u8; 4096];
                    let n = socket.read(&mut req).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&req[..n]).to_ascii_lowercase();
                    let from = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                    let (head, part) = match (mode, from) {
                        (Mode::RedirectTo(p), _) => (
                            format!(
                                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{p}/f\r\nContent-Length: 0\r\n\r\n"
                            ),
                            Vec::new(),
                        ),
                        (Mode::Ranges, Some(f)) => (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {f}-{}/{}\r\nContent-Length: {}\r\n\r\n",
                                body.len() - 1,
                                body.len(),
                                body.len() - f
                            ),
                            body[f..].to_vec(),
                        ),
                        (Mode::Breaks, _) => (
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()),
                            body[..body.len() / 2].to_vec(),
                        ),
                        _ => (
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()),
                            body.clone(),
                        ),
                    };
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&part).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        port
    }

    pub(crate) fn source_for(body: &[u8], port: u16) -> Source {
        Source {
            url: format!("http://127.0.0.1:{port}/f"),
            size: body.len() as u64,
            sha256: hex(&Sha256::digest(body)),
        }
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn local() -> Vec<String> {
        vec!["127.0.0.1".into()]
    }

    #[tokio::test]
    async fn resumes_from_the_part_file() {
        let body = body();
        let port = serve(body.clone(), Mode::Ranges).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        std::fs::write(part_name(&target), &body[..70_000]).unwrap();
        let mut first = None;
        Fetcher::for_tests(&local())
            .fetch(
                &source_for(&body, port),
                &target,
                &CancellationToken::new(),
                &mut |n| {
                    first.get_or_insert(n);
                },
            )
            .await
            .unwrap();
        assert_eq!(first, Some(70_000));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        assert!(!part_name(&target).exists());
    }

    #[tokio::test]
    async fn a_server_that_ignores_ranges_starts_over() {
        let body = body();
        let port = serve(body.clone(), Mode::IgnoresRanges).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        std::fs::write(part_name(&target), &body[..70_000]).unwrap();
        Fetcher::for_tests(&local())
            .fetch(
                &source_for(&body, port),
                &target,
                &CancellationToken::new(),
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), body);
    }

    #[tokio::test]
    async fn a_break_keeps_the_part_and_the_next_try_finishes() {
        let body = body();
        let broken = serve(body.clone(), Mode::Breaks).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        let fetcher = Fetcher::for_tests(&local());
        let cancel = CancellationToken::new();
        assert!(
            fetcher
                .fetch(&source_for(&body, broken), &target, &cancel, &mut |_| {})
                .await
                .is_err()
        );
        assert_eq!(file_len(&part_name(&target)), 100_000);
        let good = serve(body.clone(), Mode::Ranges).await;
        fetcher
            .fetch(&source_for(&body, good), &target, &cancel, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), body);
    }

    #[tokio::test]
    async fn a_wrong_hash_deletes_the_file() {
        let body = body();
        let port = serve(body.clone(), Mode::Ranges).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        let mut source = source_for(&body, port);
        source.sha256 = "0".repeat(64);
        let err = Fetcher::for_tests(&local())
            .fetch(&source, &target, &CancellationToken::new(), &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Rejected { .. }));
        assert!(!target.exists() && !part_name(&target).exists());
    }

    #[tokio::test]
    async fn redirects_only_go_to_allowed_hosts() {
        let body = body();
        let file = serve(body.clone(), Mode::Ranges).await;
        let redirect = serve(Vec::new(), Mode::RedirectTo(file)).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        let mut source = source_for(&body, redirect);
        // "localhost" is allowed, but the redirect goes to 127.0.0.1.
        source.url = source.url.replace("127.0.0.1", "localhost");
        let only_localhost = Fetcher::for_tests(&["localhost".into()]);
        assert!(
            only_localhost
                .fetch(&source, &target, &CancellationToken::new(), &mut |_| {})
                .await
                .is_err()
        );
        let both = Fetcher::for_tests(&["localhost".into(), "127.0.0.1".into()]);
        both.fetch(&source, &target, &CancellationToken::new(), &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), body);
    }

    #[tokio::test]
    async fn the_real_client_refuses_plain_http() {
        let body = body();
        let port = serve(body.clone(), Mode::Ranges).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("e.zip");
        let real = Fetcher::new(&local()).unwrap();
        assert!(
            real.fetch(
                &source_for(&body, port),
                &target,
                &CancellationToken::new(),
                &mut |_| {}
            )
            .await
            .is_err()
        );
        assert!(!target.exists());
    }

    #[test]
    fn a_dot_entry_allows_the_hosts_under_it() {
        let allowed = vec!["github.com".to_owned(), ".hf.co".to_owned()];
        assert!(host_allowed(&allowed, "GitHub.com"));
        assert!(host_allowed(&allowed, "us.aws.cdn.hf.co"));
        assert!(!host_allowed(&allowed, "hf.co.evil.example"));
        assert!(!host_allowed(&allowed, "evilhf.co"));
        assert!(!host_allowed(&allowed, "api.github.com"));
    }

    #[test]
    fn reads_the_start_of_a_content_range() {
        assert_eq!(range_start("bytes 100-999/1000"), Some(100));
        assert_eq!(range_start("bytes */1000"), None);
    }
}
