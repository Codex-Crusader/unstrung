//! The password of encrypted backups. Argon2id makes the key from the
//! password, so each guess of a stolen backup costs memory and time. Each
//! file in the zip is AES-256-GCM in 1 MiB chunks; the file name, the chunk
//! number and a last-chunk mark are in the tag, so no chunk or file can be
//! swapped, cut off or moved.

use crate::error::{Error, Result};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The shortest password that Settings accepts.
pub const MIN_PASSWORD_CHARS: usize = 8;

const KDF: &str = "argon2id";
/// About half a second and 64 MiB on a normal PC.
const MEMORY_KIB: u32 = 64 * 1024;
const PASSES: u32 = 3;
/// A backup from someone else must not make this app use a lot of memory or time.
const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_PASSES: u32 = 10;

const CHUNK: usize = 1 << 20;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const BLOCK: usize = NONCE_LEN + CHUNK + TAG_LEN;
/// The text sealed into the manifest: it opens only with the right password.
const CHECK: &[u8] = b"unstrung backup password check";

/// In the manifest of an encrypted backup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Encryption {
    pub kdf: String,
    pub memory_kib: u32,
    pub passes: u32,
    pub salt: Vec<u8>,
    /// `CHECK`, sealed, so a wrong password shows as wrong, not as damage.
    pub check: Vec<u8>,
}

pub struct BackupKey(Aes256Gcm);

fn random(out: &mut [u8]) -> Result<()> {
    getrandom::fill(out).map_err(|e| Error::KeyLost(format!("no random bytes: {e}")))
}

fn derive(password: &str, salt: &[u8], memory_kib: u32, passes: u32) -> Result<BackupKey> {
    let params = argon2::Params::new(memory_kib, passes, 1, Some(32))
        .map_err(|e| Error::Invalid(format!("the backup password settings are wrong ({e})")))?;
    let mut key = [0u8; 32];
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| Error::Invalid(format!("the backup password did not work ({e})")))?;
    Ok(BackupKey(Aes256Gcm::new(&key.into())))
}

fn wrong_password() -> Error {
    Error::WrongPassword
}

fn damaged(name: &str) -> Error {
    Error::Invalid(format!(
        "this file is not a backup that Unstrung can read: {name} is damaged"
    ))
}

/// What the tag covers besides the bytes: the file, the chunk and whether
/// it is the last one.
fn aad(name: &str, index: u64, last: bool) -> Vec<u8> {
    let mut aad = Vec::with_capacity(name.len() + 9);
    aad.extend_from_slice(name.as_bytes());
    aad.extend_from_slice(&index.to_le_bytes());
    aad.push(u8::from(last));
    aad
}

/// Fills `buf` from `from` as far as it goes. Returns the bytes read.
fn fill(from: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match from.read(&mut buf[n..])? {
            0 => break,
            m => n += m,
        }
    }
    Ok(n)
}

impl BackupKey {
    /// A new key for one export: a new salt, and the check sealed with it.
    pub fn new(password: &str) -> Result<(Self, Encryption)> {
        let mut salt = vec![0u8; 16];
        random(&mut salt)?;
        let key = derive(password, &salt, MEMORY_KIB, PASSES)?;
        let mut check = Vec::new();
        key.seal("check", &mut &CHECK[..], &mut check)?;
        Ok((
            key,
            Encryption {
                kdf: KDF.into(),
                memory_kib: MEMORY_KIB,
                passes: PASSES,
                salt,
                check,
            },
        ))
    }

    /// The key of a backup, if `password` is right.
    pub fn unlock(password: &str, e: &Encryption) -> Result<Self> {
        if e.kdf != KDF || e.memory_kib > MAX_MEMORY_KIB || e.passes > MAX_PASSES {
            return Err(Error::Invalid(
                "this backup comes from a newer version of Unstrung. Update Unstrung first".into(),
            ));
        }
        let key = derive(password, &e.salt, e.memory_kib, e.passes)?;
        key.verify(e)?;
        Ok(key)
    }

    /// True when this key made `e`: the check opens.
    pub fn verify(&self, e: &Encryption) -> Result<()> {
        let mut out = Vec::new();
        self.open(
            "check",
            &mut e.check.as_slice(),
            e.check.len() as u64,
            &mut out,
        )
        .ok()
        .filter(|()| out == CHECK)
        .ok_or_else(wrong_password)
    }

    /// Encrypts everything from `from` into `to` as the zip file `name`.
    pub fn seal(&self, name: &str, from: &mut impl Read, to: &mut impl Write) -> Result<()> {
        let mut chunk = vec![0u8; CHUNK];
        // One chunk ahead, so the last one is known when it is sealed.
        let mut len = fill(from, &mut chunk)?;
        let mut index = 0u64;
        loop {
            let mut next = vec![0u8; CHUNK];
            let next_len = if len == CHUNK {
                fill(from, &mut next)?
            } else {
                0
            };
            let last = next_len == 0;
            let mut nonce = [0u8; NONCE_LEN];
            random(&mut nonce)?;
            let sealed = self
                .0
                .encrypt(
                    &Nonce::from(nonce),
                    Payload {
                        msg: &chunk[..len],
                        aad: &aad(name, index, last),
                    },
                )
                .map_err(|_| Error::Damaged("a backup file could not be encrypted".into()))?;
            to.write_all(&nonce)?;
            to.write_all(&sealed)?;
            if last {
                return Ok(());
            }
            (chunk, len) = (next, next_len);
            index += 1;
        }
    }

    /// Decrypts the zip file `name` of `size` bytes from `from` into `to`.
    pub fn open(
        &self,
        name: &str,
        from: &mut impl Read,
        size: u64,
        to: &mut impl Write,
    ) -> Result<()> {
        let size = usize::try_from(size).map_err(|_| damaged(name))?;
        let blocks = size.div_ceil(BLOCK).max(1);
        let mut block = vec![0u8; BLOCK];
        for index in 0..blocks {
            let len = fill(from, &mut block)?;
            let expected = if index + 1 == blocks {
                size - index * BLOCK
            } else {
                BLOCK
            };
            if len != expected || len < NONCE_LEN + TAG_LEN {
                return Err(damaged(name));
            }
            let (nonce, sealed) = block[..len].split_at(NONCE_LEN);
            let nonce = Nonce::try_from(nonce).map_err(|_| damaged(name))?;
            let plain = self
                .0
                .decrypt(
                    &nonce,
                    Payload {
                        msg: sealed,
                        aad: &aad(name, index as u64, index + 1 == blocks),
                    },
                )
                .map_err(|_| damaged(name))?;
            to.write_all(&plain)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed(key: &BackupKey, name: &str, plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        key.seal(name, &mut &plain[..], &mut out).unwrap();
        out
    }

    fn opened(key: &BackupKey, name: &str, data: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        key.open(name, &mut &data[..], data.len() as u64, &mut out)
            .map(|()| out)
    }

    #[test]
    fn files_of_any_size_come_back_and_changes_fail() {
        let (key, enc) = BackupKey::new("long enough").unwrap();
        for size in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK + 7] {
            let plain: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let data = sealed(&key, "gallery/1/a.png", &plain);
            assert_eq!(
                opened(&key, "gallery/1/a.png", &data).unwrap(),
                plain,
                "{size}"
            );
            assert!(opened(&key, "gallery/1/b.png", &data).is_err(), "renamed");
            if size > CHUNK {
                assert!(
                    opened(&key, "gallery/1/a.png", &data[..BLOCK]).is_err(),
                    "cut off"
                );
            }
            let mut changed = data.clone();
            let at = changed.len() / 2;
            changed[at] ^= 1;
            assert!(
                opened(&key, "gallery/1/a.png", &changed).is_err(),
                "changed"
            );
        }
        assert!(BackupKey::unlock("long enough", &enc).is_ok());
        assert_eq!(
            BackupKey::unlock("long enougH", &enc)
                .err()
                .map(|e| e.to_string()),
            Some("the password is wrong".into())
        );
    }

    #[test]
    fn settings_from_a_strange_backup_are_refused() {
        let (_, mut enc) = BackupKey::new("long enough").unwrap();
        enc.memory_kib = MAX_MEMORY_KIB + 1;
        assert!(BackupKey::unlock("long enough", &enc).is_err());
    }
}
