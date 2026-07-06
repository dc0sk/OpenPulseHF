//! A uniform secret-store interface (REQ-SEC-CTL-03) over two backends: the OS system secret
//! store (keychain / Secret Service / Credential Manager, feature `keychain`) and the
//! master-password [`FileKeystore`](crate::FileKeystore) fallback for headless hosts.

use std::path::{Path, PathBuf};

use crate::{FileKeystore, KeystoreError};

/// Read/write named secrets. `key_id` names a secret within a service namespace.
pub trait SecretStore {
    /// The secret for `key_id`, or `None` if absent.
    fn get(&self, key_id: &str) -> Result<Option<Vec<u8>>, KeystoreError>;
    /// Insert or replace a secret (persisted immediately).
    fn set(&mut self, key_id: &str, secret: &[u8]) -> Result<(), KeystoreError>;
    /// Remove a secret (a no-op if absent).
    fn delete(&mut self, key_id: &str) -> Result<(), KeystoreError>;
}

/// A [`SecretStore`] backed by the master-password [`FileKeystore`]; each mutation re-saves.
pub struct FileStore {
    inner: FileKeystore,
}

impl FileStore {
    /// Open the keystore at `path` (creating an empty one if absent) under `master`.
    pub fn open(path: impl Into<PathBuf>, master: &str) -> Result<Self, KeystoreError> {
        let path = path.into();
        let inner = if path.exists() {
            FileKeystore::open(path, master)?
        } else {
            FileKeystore::create(path, master)
        };
        Ok(Self { inner })
    }

    /// The keystore file path.
    pub fn path(&self) -> &Path {
        self.inner.path()
    }
}

impl SecretStore for FileStore {
    fn get(&self, key_id: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        Ok(self.inner.get(key_id).map(|b| b.to_vec()))
    }

    fn set(&mut self, key_id: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        self.inner.set(key_id, secret.to_vec());
        self.inner.save()
    }

    fn delete(&mut self, key_id: &str) -> Result<(), KeystoreError> {
        self.inner.remove(key_id);
        self.inner.save()
    }
}

/// A [`SecretStore`] backed by the operating system's secret service, keyed by `(service, key_id)`.
#[cfg(feature = "keychain")]
pub struct KeychainStore {
    service: String,
}

#[cfg(feature = "keychain")]
impl KeychainStore {
    /// A keychain store under the given service namespace (e.g. `"openpulse"`).
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    /// Best-effort probe: `true` if the platform secret service is reachable (so a caller can fall
    /// back to a [`FileStore`] on a headless host). A missing entry counts as reachable.
    pub fn available(&self) -> bool {
        match keyring::Entry::new(&self.service, "__openpulse_probe__") {
            Ok(entry) => !matches!(
                entry.get_secret(),
                Err(keyring::Error::NoStorageAccess(_)) | Err(keyring::Error::PlatformFailure(_))
            ),
            Err(_) => false,
        }
    }
}

#[cfg(feature = "keychain")]
fn kc(err: keyring::Error) -> KeystoreError {
    KeystoreError::Keychain(err.to_string())
}

#[cfg(feature = "keychain")]
impl SecretStore for KeychainStore {
    fn get(&self, key_id: &str) -> Result<Option<Vec<u8>>, KeystoreError> {
        let entry = keyring::Entry::new(&self.service, key_id).map_err(kc)?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(kc(err)),
        }
    }

    fn set(&mut self, key_id: &str, secret: &[u8]) -> Result<(), KeystoreError> {
        keyring::Entry::new(&self.service, key_id)
            .map_err(kc)?
            .set_secret(secret)
            .map_err(kc)
    }

    fn delete(&mut self, key_id: &str) -> Result<(), KeystoreError> {
        let entry = keyring::Entry::new(&self.service, key_id).map_err(kc)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(kc(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_get_set_delete_round_trip() {
        let path = std::env::temp_dir().join(format!("openpulse-store-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut store = FileStore::open(&path, "master-pw").unwrap();
        assert!(store.get("control-psk").unwrap().is_none());
        store.set("control-psk", &[1, 2, 3, 4]).unwrap();

        // Re-open to prove it persisted through the master password.
        let reopened = FileStore::open(&path, "master-pw").unwrap();
        assert_eq!(reopened.get("control-psk").unwrap(), Some(vec![1, 2, 3, 4]));

        let mut store = FileStore::open(&path, "master-pw").unwrap();
        store.delete("control-psk").unwrap();
        assert!(store.get("control-psk").unwrap().is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "keychain")]
    #[test]
    #[ignore = "requires a running OS secret service; run manually"]
    fn keychain_round_trip() {
        let mut store = KeychainStore::new("openpulse-test");
        if !store.available() {
            return;
        }
        store.set("probe-key", &[9, 8, 7]).unwrap();
        assert_eq!(store.get("probe-key").unwrap(), Some(vec![9, 8, 7]));
        store.delete("probe-key").unwrap();
        assert!(store.get("probe-key").unwrap().is_none());
    }
}
