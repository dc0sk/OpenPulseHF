//! Secret storage for OpenPulse (REQ-SEC-CTL-04).
//!
//! [`FileKeystore`] stores named secrets encrypted at rest under an operator master password
//! (Argon2id KDF → ChaCha20-Poly1305 AEAD). The master password is held only in memory and is
//! never written to disk. The keystore file is owner-only (via `openpulse_config::secret_file`).
//!
//! File layout (version 1): `b"OPKS"` | `0x01` | salt(16) | nonce(12) | ciphertext(+16-byte tag).
//! The plaintext is a JSON map of `key-id → secret bytes`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use argon2::Argon2;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand::RngCore;
use thiserror::Error;

const MAGIC: &[u8; 4] = b"OPKS";
const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN;

/// Errors from the file keystore.
#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("keystore io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("keystore permission error: {0}")]
    Permissions(#[from] openpulse_config::ConfigError),
    #[error("not a valid keystore file (bad magic/version or truncated)")]
    Format,
    #[error("wrong master password or corrupt keystore")]
    Decrypt,
    #[error("key derivation failed")]
    Kdf,
    #[error("cipher error")]
    Cipher,
    #[error("keystore payload error: {0}")]
    Payload(#[from] serde_json::Error),
}

/// A master-password-encrypted store of named secrets, persisted to a single file.
pub struct FileKeystore {
    path: PathBuf,
    master: String,
    secrets: BTreeMap<String, Vec<u8>>,
}

impl FileKeystore {
    /// A new, empty keystore held in memory. Call [`save`](Self::save) to encrypt and write it.
    pub fn create(path: impl Into<PathBuf>, master: &str) -> Self {
        Self {
            path: path.into(),
            master: master.to_string(),
            secrets: BTreeMap::new(),
        }
    }

    /// Open and decrypt an existing keystore with `master`. Refuses a group/world-readable file
    /// (REQ-SEC-CTL-05) and returns [`KeystoreError::Decrypt`] on a wrong password or tampering.
    pub fn open(path: impl Into<PathBuf>, master: &str) -> Result<Self, KeystoreError> {
        let path = path.into();
        openpulse_config::secret_file::validate_owner_only(&path)?;
        let raw = std::fs::read(&path)?;
        if raw.len() < HEADER_LEN || &raw[0..4] != MAGIC || raw[4] != VERSION {
            return Err(KeystoreError::Format);
        }
        let salt = &raw[5..5 + SALT_LEN];
        let nonce = &raw[5 + SALT_LEN..HEADER_LEN];
        let ciphertext = &raw[HEADER_LEN..];
        let key = derive_key(master, salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| KeystoreError::Cipher)?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| KeystoreError::Decrypt)?;
        let secrets: BTreeMap<String, Vec<u8>> = serde_json::from_slice(&plaintext)?;
        Ok(Self {
            path,
            master: master.to_string(),
            secrets,
        })
    }

    /// The stored secret for `key_id`, if present.
    pub fn get(&self, key_id: &str) -> Option<&[u8]> {
        self.secrets.get(key_id).map(|v| v.as_slice())
    }

    /// Insert or replace a secret.
    pub fn set(&mut self, key_id: &str, secret: Vec<u8>) {
        self.secrets.insert(key_id.to_string(), secret);
    }

    /// Remove a secret, returning it if present.
    pub fn remove(&mut self, key_id: &str) -> Option<Vec<u8>> {
        self.secrets.remove(key_id)
    }

    /// The keystore file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Encrypt and write the keystore to disk with owner-only permissions, using a fresh random
    /// salt + nonce each time.
    pub fn save(&self) -> Result<(), KeystoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut salt = [0u8; SALT_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let key = derive_key(&self.master, &salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| KeystoreError::Cipher)?;
        let plaintext = serde_json::to_vec(&self.secrets)?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_slice())
            .map_err(|_| KeystoreError::Cipher)?;

        let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);

        write_owner_only(&self.path, &out)?;
        openpulse_config::secret_file::enforce_owner_only(&self.path)?;
        Ok(())
    }
}

/// Derive a 32-byte key from the master password + salt with Argon2id (default params).
fn derive_key(master: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], KeystoreError> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(master.as_bytes(), salt, &mut key)
        .map_err(|_| KeystoreError::Kdf)?;
    Ok(key)
}

#[cfg(unix)]
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("openpulse-ks-{tag}-{}", std::process::id()))
    }

    #[test]
    fn round_trip_with_correct_master() {
        let p = tmp("rt");
        let _ = std::fs::remove_file(&p);
        let mut ks = FileKeystore::create(&p, "correct horse battery staple");
        ks.set("control-psk", vec![0xAB; 32]);
        ks.set("other", b"hello".to_vec());
        ks.save().unwrap();

        let ks2 = FileKeystore::open(&p, "correct horse battery staple").unwrap();
        assert_eq!(ks2.get("control-psk"), Some(&[0xAB; 32][..]));
        assert_eq!(ks2.get("other"), Some(&b"hello"[..]));
        assert_eq!(ks2.get("missing"), None);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn wrong_master_is_rejected() {
        let p = tmp("wrong");
        let _ = std::fs::remove_file(&p);
        let mut ks = FileKeystore::create(&p, "right");
        ks.set("k", vec![1, 2, 3]);
        ks.save().unwrap();
        assert!(matches!(
            FileKeystore::open(&p, "WRONG"),
            Err(KeystoreError::Decrypt)
        ));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let p = tmp("tamper");
        let _ = std::fs::remove_file(&p);
        let mut ks = FileKeystore::create(&p, "m");
        ks.set("k", vec![9; 16]);
        ks.save().unwrap();
        let mut raw = std::fs::read(&p).unwrap();
        let n = raw.len();
        raw[n - 1] ^= 0xFF; // flip the last (tag) byte
        std::fs::write(&p, &raw).unwrap();
        assert!(matches!(
            FileKeystore::open(&p, "m"),
            Err(KeystoreError::Decrypt)
        ));
        let _ = std::fs::remove_file(&p);
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("perm");
        let _ = std::fs::remove_file(&p);
        let mut ks = FileKeystore::create(&p, "m");
        ks.set("k", vec![1]);
        ks.save().unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&p);
    }
}
