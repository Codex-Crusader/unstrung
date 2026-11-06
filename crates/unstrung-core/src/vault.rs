//! The data key. The chats file and the picture files are encrypted with
//! keys made from one random 32-byte key. That key lives in
//! `<app folder>\data.key`, wrapped by Windows (DPAPI) for the current
//! Windows user. Another Windows user, another PC, or a copied disk cannot
//! unwrap it. A program that runs as this Windows user can.
//!
//! The key is never made again for data that already exists: a key that
//! does not unwrap (for example after an administrator reset the Windows
//! password) is an error, and the data stays as it is.

use crate::error::{Error, Result};
use crate::paths::AppPaths;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use std::path::{Path, PathBuf};

/// The first bytes of an encrypted picture file. The last byte is the
/// format version.
const MAGIC: &[u8; 8] = b"UNSTRNG\x01";
const NONCE_LEN: usize = 12;
const DAMAGED_FILE: &str = "this picture is damaged, or it was saved with another key";
/// The first bytes of a plain SQLite file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// The key, in memory only.
#[derive(Clone)]
pub struct DataKey {
    master: [u8; 32],
}

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(hidden)")
    }
}

impl DataKey {
    /// A key that is not stored anywhere, for tests.
    pub fn random() -> Result<Self> {
        let mut master = [0u8; 32];
        getrandom::fill(&mut master)
            .map_err(|e| Error::KeyLost(format!("no random bytes: {e}")))?;
        Ok(Self { master })
    }

    fn derive(&self, purpose: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        // 32 bytes is far below the HKDF limit, so expand cannot fail.
        let _ = Hkdf::<Sha256>::new(None, &self.master).expand(purpose, &mut out);
        out
    }

    /// The SQLCipher key as a raw key, `x'<64 hex digits>'`. A raw key
    /// skips the slow password step on every open.
    pub fn database_key(&self) -> String {
        let hex: String = self
            .derive(b"unstrung database")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("x'{hex}'")
    }

    /// One fixed key, so a test can open the same file twice.
    #[cfg(test)]
    pub(crate) fn test() -> Self {
        Self { master: [7; 32] }
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(&self.derive(b"unstrung files").into())
    }

    /// Encrypts the bytes of a file: magic, nonce, then the ciphertext with
    /// its tag.
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| Error::KeyLost(format!("no random bytes: {e}")))?;
        let sealed = self
            .cipher()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: MAGIC,
                },
            )
            .map_err(|_| Error::Damaged("a file could not be encrypted".into()))?;
        let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// The plain bytes of a file. A file from before encryption comes back
    /// as it is, so old files keep working until they are converted.
    pub fn open(&self, data: &[u8]) -> Result<Vec<u8>> {
        if !is_sealed(data) {
            return Ok(data.to_vec());
        }
        let rest = &data[MAGIC.len()..];
        if rest.len() < NONCE_LEN {
            return Err(Error::Invalid(DAMAGED_FILE.into()));
        }
        let (nonce, sealed) = rest.split_at(NONCE_LEN);
        self.cipher()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: MAGIC,
                },
            )
            .map_err(|_| Error::Invalid(DAMAGED_FILE.into()))
    }

    /// Reads and decrypts a file.
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.open(&std::fs::read(path)?)
    }

    /// Encrypts and writes a file, atomically.
    pub fn write(&self, path: &Path, plain: &[u8]) -> Result<()> {
        crate::files::write_atomic(path, &self.seal(plain)?)?;
        Ok(())
    }
}

/// True for the bytes of an encrypted file.
pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

/// True when the file at `path` is a plain SQLite database.
pub fn is_plain_database(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_ok_and(|()| &header == SQLITE_HEADER)
}

/// The files in `dirs` and one level below: the avatars, and the gallery
/// with a folder per chat.
fn picture_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let entries = |d: &Path| {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect::<Vec<_>>()
    };
    dirs.iter()
        .flat_map(|d| entries(d))
        .flat_map(|p| if p.is_dir() { entries(&p) } else { vec![p] })
        .filter(|p| p.is_file())
        .collect()
}

/// The folders that hold pictures: the live ones, and the aside copies of
/// a backup Replace (`backups/replaced-import-*/avatars`,
/// `<library>/gallery-replaced-*`).
fn picture_dirs(paths: &AppPaths) -> Vec<PathBuf> {
    let aside = |dir: PathBuf, prefix: &str| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect::<Vec<_>>()
    };
    let mut dirs = vec![paths.avatars(), paths.gallery()];
    dirs.extend(
        aside(paths.backups(), "replaced-import-")
            .into_iter()
            .map(|d| d.join("avatars")),
    );
    dirs.extend(aside(paths.library(), "gallery-replaced-"));
    dirs
}

fn file_is_sealed(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; MAGIC.len()];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| &head == MAGIC)
}

fn first_sealed_file(dirs: &[PathBuf]) -> Option<PathBuf> {
    picture_files(dirs).into_iter().find(|p| file_is_sealed(p))
}

/// Encrypts the pictures saved before encryption. Safe to stop and run
/// again: each file is replaced atomically, and a sealed file is skipped.
/// Returns how many files it encrypted.
pub fn seal_old_pictures(paths: &AppPaths) -> Result<usize> {
    let key = paths.key()?;
    let mut sealed = 0;
    for file in picture_files(&picture_dirs(paths)) {
        if file_is_sealed(&file) {
            continue;
        }
        // shortcut: a picture deleted during this pass comes back as an
        // orphan file without a row; harmless, and only once per install.
        match std::fs::read(&file) {
            Ok(bytes) => key.write(&file, &bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        }
        sealed += 1;
    }
    Ok(sealed)
}

fn key_file(paths: &AppPaths) -> PathBuf {
    paths.root().join("data.key")
}

/// The key of this app folder. Makes one only when no encrypted data exists
/// yet; otherwise a missing or broken key is an error.
pub fn load_or_create(paths: &AppPaths) -> Result<DataKey> {
    let file = key_file(paths);
    if file.exists() {
        let wrapped = std::fs::read(&file)?;
        let master = unwrap(&wrapped)?;
        let master: [u8; 32] = master
            .try_into()
            .map_err(|_| Error::KeyLost("data.key has the wrong length".into()))?;
        return Ok(DataKey { master });
    }
    let db = paths.database();
    if db.exists() && !is_plain_database(&db) {
        return Err(Error::KeyLost(
            "the chats file is encrypted, but data.key is missing".into(),
        ));
    }
    if let Some(file) = first_sealed_file(&[paths.avatars(), paths.gallery()]) {
        return Err(Error::KeyLost(format!(
            "{} is encrypted, but data.key is missing",
            file.display()
        )));
    }
    let key = DataKey::random()?;
    crate::files::write_atomic(&file, &wrap(&key.master)?)?;
    log::info!("made a new data key");
    Ok(key)
}

#[cfg(windows)]
fn wrap(secret: &[u8]) -> Result<Vec<u8>> {
    windows_dpapi::encrypt_data(secret, windows_dpapi::Scope::User, Some(b"unstrung"))
        .map_err(|e| Error::KeyLost(format!("Windows did not protect the key: {e:#}")))
}

#[cfg(windows)]
fn unwrap(wrapped: &[u8]) -> Result<Vec<u8>> {
    windows_dpapi::decrypt_data(wrapped, windows_dpapi::Scope::User, Some(b"unstrung"))
        .map_err(|e| Error::KeyLost(format!("Windows did not open the key: {e:#}")))
}

#[cfg(not(windows))]
fn wrap(_: &[u8]) -> Result<Vec<u8>> {
    Err(Error::KeyLost("the data key needs Windows".into()))
}

#[cfg(not(windows))]
fn unwrap(_: &[u8]) -> Result<Vec<u8>> {
    Err(Error::KeyLost("the data key needs Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_file_opens_only_with_its_key() {
        let key = DataKey::random().unwrap();
        let sealed = key.seal(b"\x89PNG picture").unwrap();
        assert!(is_sealed(&sealed));
        assert!(!sealed.windows(3).any(|w| w == b"PNG"));
        assert_eq!(key.open(&sealed).unwrap(), b"\x89PNG picture");
        let other = DataKey::random().unwrap();
        assert!(other.open(&sealed).is_err());
        let mut broken = sealed.clone();
        let last = broken.len() - 1;
        broken[last] ^= 1;
        assert!(key.open(&broken).is_err());
        assert_eq!(key.open(b"\x89PNG old").unwrap(), b"\x89PNG old");
    }

    #[cfg(windows)]
    #[test]
    fn old_pictures_are_sealed_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.gallery().join("7")).unwrap();
        std::fs::create_dir_all(paths.avatars()).unwrap();
        std::fs::write(paths.gallery().join("7").join("a.png"), b"PNG a").unwrap();
        std::fs::write(paths.avatars().join("b.png"), b"PNG b").unwrap();
        let aside = paths.backups().join("replaced-import-9").join("avatars");
        std::fs::create_dir_all(&aside).unwrap();
        std::fs::write(aside.join("c.png"), b"PNG c").unwrap();
        assert_eq!(seal_old_pictures(&paths).unwrap(), 3);
        assert_eq!(seal_old_pictures(&paths).unwrap(), 0);
        let raw = std::fs::read(paths.avatars().join("b.png")).unwrap();
        assert!(is_sealed(&raw));
        assert_eq!(paths.key().unwrap().open(&raw).unwrap(), b"PNG b");
    }

    #[test]
    fn database_and_file_keys_differ() {
        let key = DataKey::random().unwrap();
        assert_eq!(key.database_key().len(), 67);
        assert_ne!(
            key.derive(b"unstrung database"),
            key.derive(b"unstrung files")
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_key_survives_a_restart_and_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        let first = load_or_create(&paths).unwrap();
        let again = load_or_create(&paths).unwrap();
        assert_eq!(first.master, again.master);
        std::fs::write(key_file(&paths), b"garbage").unwrap();
        assert!(matches!(load_or_create(&paths), Err(Error::KeyLost(_))));
        std::fs::remove_file(key_file(&paths)).unwrap();
        std::fs::create_dir_all(paths.gallery().join("4")).unwrap();
        std::fs::write(
            paths.gallery().join("4").join("p.png"),
            first.seal(b"p").unwrap(),
        )
        .unwrap();
        assert!(matches!(load_or_create(&paths), Err(Error::KeyLost(_))));
        std::fs::remove_dir_all(paths.gallery()).unwrap();
        std::fs::write(paths.database(), b"not a plain sqlite file....").unwrap();
        assert!(matches!(load_or_create(&paths), Err(Error::KeyLost(_))));
        assert!(!key_file(&paths).exists(), "no new key over encrypted data");
    }
}
