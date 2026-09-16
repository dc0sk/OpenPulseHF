//! A uniform secret-store interface over two backends: the OS system secret store (keychain /
//! Secret Service / Credential Manager, feature `keychain`) and the master-password
//! [`FileKeystore`](crate::FileKeystore) fallback for headless hosts.
//!
//! **The OS-store half is a manual-only surface.** Its requirement id was retired from the registry
//! in #1234: `KeychainStore` is behind the `keychain` feature, which the gate's
//! `--no-default-features` drops, so nothing here can be run-confirmed by any tier this repo has.
//! Its only evidence is the `#[ignore]`d `keychain_round_trip`. The *selection* between the two
//! backends is REQ-CTL-04's fallback clause and IS gate-verified — see [`select_backend`].

use std::path::{Path, PathBuf};

use crate::{FileKeystore, KeystoreError};

/// Which backend a caller should open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The OS system secret store.
    Keychain,
    /// The master-password file keystore.
    File,
}

/// What is known about the OS secret store, as an INPUT to [`select_backend`].
///
/// **Three states, not a boolean, and the third is why.** A boolean cannot distinguish "this host
/// has no secret service" from "this BUILD has no keychain support" — and the second is the case
/// that actually ships, because the gate builds `--no-default-features`. Collapsing them would make
/// the selector untestable in exactly the configuration the gate runs (#1234).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainProbe {
    /// The `keychain` feature is off, so no OS backend exists in this binary at all.
    NotCompiledIn,
    /// Compiled in, but the platform secret service did not answer.
    Unreachable,
    /// Compiled in and reachable.
    Reachable,
}

/// Choose a backend from what is known about the OS store.
///
/// **No I/O — the probe is supplied**, which is what makes this verifiable on a headless host and in
/// the `--no-default-features` build the gate actually runs. Deliberately NOT behind
/// `cfg(feature = "keychain")`: a selector compiled only when the feature is on is never
/// type-checked by the gate, which is #1380's shape.
///
/// The fallback is never silent. A caller that lands on [`Backend::File`] when it wanted the OS
/// store gets a reason from [`fallback_reason`], because an operator who believed their secrets were
/// in the system keychain and finds them in a file has been misled by the absence of a message.
///
/// **Exported from the crate root like the rest of this crate's API**, and for the same reason: it
/// IS the contract a consumer will use. `pub(crate)` was tried first and is wrong here — unlike
/// #1310 PR1b, where a production function called the new one, nothing inside this crate calls
/// these, so `pub(crate)` makes them dead code in a non-test build. `openpulse-keystore` has no
/// dependent crate at all yet (#1234); `KeychainStore`, `FileStore` and `SecretStore` sit in exactly
/// the same position and are handled exactly this way.
pub fn select_backend(probe: KeychainProbe) -> Backend {
    match probe {
        KeychainProbe::Reachable => Backend::Keychain,
        KeychainProbe::NotCompiledIn | KeychainProbe::Unreachable => Backend::File,
    }
}

/// Why the file backend was chosen, or `None` when the OS store was used.
///
/// Separated from [`select_backend`] so the *message* is as testable as the *choice*: a selector
/// that falls back correctly but says nothing is the fail-open this repo refuses elsewhere.
pub fn fallback_reason(probe: KeychainProbe) -> Option<&'static str> {
    match probe {
        KeychainProbe::Reachable => None,
        KeychainProbe::NotCompiledIn => {
            Some("the OS secret store is not compiled in (feature `keychain` is off)")
        }
        KeychainProbe::Unreachable => {
            Some("the OS secret store is compiled in but unreachable on this host")
        }
    }
}

/// Probe the OS secret store, resolving to [`KeychainProbe::NotCompiledIn`] when the feature is off.
///
/// Modelled on the daemon's `build_audio_backend`: always compiled, with the feature arm INSIDE, so
/// the absent-feature path is a value this function returns rather than a function that vanishes.
pub fn probe_keychain(_service: &str) -> KeychainProbe {
    #[cfg(feature = "keychain")]
    {
        if KeychainStore::new(_service).available() {
            return KeychainProbe::Reachable;
        }
        KeychainProbe::Unreachable
    }
    #[cfg(not(feature = "keychain"))]
    {
        KeychainProbe::NotCompiledIn
    }
}

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
mod selection {
    //! REQ-CTL-04's FALLBACK clause: the file keystore is available for hosts without a usable
    //! system secret store (#1234).
    //!
    //! **Why this is a real gate and not a truth table.** The interesting state is
    //! `NotCompiledIn`, and it is the state the GATE ITSELF runs in: `--no-default-features` drops
    //! the `keychain` feature, so `probe_keychain` returns it and the shipped binary really does
    //! take the fallback. A boolean probe could not express that — it would conflate "no secret
    //! service on this host" with "no keychain in this build" — and the test would then assert
    //! nothing about the binary the gate produces.
    //!
    //! **What this does NOT cover, stated so it is not read as more:** the `KeychainStore` body
    //! itself (`new`, `available`) is compiled only with the feature, so the gate never type-checks
    //! it. That is #1380's shape, contained here rather than closed. Its evidence is the
    //! `#[ignore]`d `keychain_round_trip` only.
    use super::*;

    // VERIFIES: REQ-CTL-04 — a file keystore is available as the fallback when no usable system
    // secret store is present, and the fallback is never silent.

    #[test]
    fn an_absent_or_unreachable_os_store_falls_back_to_the_file_keystore() {
        assert_eq!(select_backend(KeychainProbe::NotCompiledIn), Backend::File);
        assert_eq!(select_backend(KeychainProbe::Unreachable), Backend::File);
    }

    #[test]
    fn a_reachable_os_store_is_preferred() {
        assert_eq!(select_backend(KeychainProbe::Reachable), Backend::Keychain);
    }

    /// The fallback must SAY why. An operator who believed their secrets were in the system keychain
    /// and finds them in a file has been misled by the absence of a message, which is the fail-open
    /// shape this repo refuses elsewhere. Delete either `Some(..)` arm and this fails.
    #[test]
    fn falling_back_always_carries_a_reason_and_preferring_never_does() {
        assert!(fallback_reason(KeychainProbe::Reachable).is_none());
        let absent = fallback_reason(KeychainProbe::NotCompiledIn).expect("a reason");
        let unreachable = fallback_reason(KeychainProbe::Unreachable).expect("a reason");
        // The two must be DISTINGUISHABLE: "not built with it" and "built but the host has none"
        // are different operator problems with different fixes.
        assert_ne!(absent, unreachable);
        assert!(
            absent.contains("not compiled in"),
            "the absent-feature reason must name the build, not the host: {absent}"
        );
    }

    /// The property that makes the above non-vacuous IN THE GATE'S OWN BUILD: with the feature off,
    /// the probe really does report `NotCompiledIn`, so the shipped `--no-default-features` binary
    /// takes the fallback path these tests describe.
    #[cfg(not(feature = "keychain"))]
    #[test]
    fn the_gates_own_build_reports_not_compiled_in() {
        assert_eq!(probe_keychain("openpulse"), KeychainProbe::NotCompiledIn);
        assert_eq!(
            select_backend(probe_keychain("openpulse")),
            Backend::File,
            "the --no-default-features binary must resolve to the file keystore"
        );
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
