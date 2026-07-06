//! Owner-only permission checks for files holding key/secret material (REQ-SEC-CTL-05).
//!
//! Used by both the daemon (server) and the CLI/panel (clients) so every secret file —
//! identity key, trust store, keystore, PSK — is validated on load and set owner-only on write.
//! On non-Unix platforms these are no-ops (documented).

use std::path::Path;

use crate::ConfigError;

/// Refuse a secret file that is group- or world-accessible.
///
/// On Unix, requires owner-only (`mode & 0o077 == 0`); returns
/// [`ConfigError::InsecureSecretPermissions`] otherwise. A no-op on other platforms.
pub fn validate_owner_only(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(ConfigError::InsecureSecretPermissions {
                path: path.display().to_string(),
                mode,
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Set owner-only (`0600`) permissions on a secret file. A no-op on non-Unix platforms.
pub fn enforce_owner_only(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("openpulse-secret-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, b"secret").unwrap();
        p
    }

    #[test]
    fn accepts_owner_only_and_rejects_group_or_world() {
        let p = tmp("perm");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(validate_owner_only(&p).is_ok(), "0600 must be accepted");

        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(
            matches!(
                validate_owner_only(&p),
                Err(ConfigError::InsecureSecretPermissions { .. })
            ),
            "group-readable must be rejected"
        );

        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o604)).unwrap();
        assert!(
            validate_owner_only(&p).is_err(),
            "world-readable must be rejected"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn enforce_sets_owner_only() {
        let p = tmp("enforce");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        enforce_owner_only(&p).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(validate_owner_only(&p).is_ok());
        let _ = std::fs::remove_file(&p);
    }
}
