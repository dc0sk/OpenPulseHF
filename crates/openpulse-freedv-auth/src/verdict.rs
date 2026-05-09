//! Authentication verdict and Unix-socket server for companion UI polling.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;

#[derive(Debug, Error)]
pub enum VerdictError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Authentication result for an incoming FreeDV session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum TrustVerdict {
    /// Signature verified against the embedded public key.
    Verified {
        callsign: String,
        /// Hex-encoded first 8 bytes of the Ed25519 public key.
        key_id: String,
        last_beacon_utc: u64,
    },
    /// No valid beacon has been received yet.
    Unverified { reason: String },
    /// A beacon was received but the signature check failed.
    Invalid { reason: String },
}

impl TrustVerdict {
    pub fn unverified(reason: impl Into<String>) -> Self {
        Self::Unverified {
            reason: reason.into(),
        }
    }

    pub fn invalid(reason: impl Into<String>) -> Self {
        Self::Invalid {
            reason: reason.into(),
        }
    }
}

impl Default for TrustVerdict {
    fn default() -> Self {
        Self::unverified("no beacon received")
    }
}

/// Shared verdict that can be updated by the receive task and read by the server.
pub type SharedVerdict = Arc<RwLock<TrustVerdict>>;

/// Unix-socket server that writes the current [`TrustVerdict`] as a single
/// JSON line to every connecting client.
pub struct VerdictServer {
    path: PathBuf,
    verdict: SharedVerdict,
}

impl VerdictServer {
    /// Create a server that will listen at `path`.
    pub fn new(path: impl Into<PathBuf>, verdict: SharedVerdict) -> Self {
        Self {
            path: path.into(),
            verdict,
        }
    }

    /// Accept connections in a loop, writing the current verdict and closing.
    ///
    /// Call this inside a [`tokio::spawn`]'d task.  The server runs until the
    /// task is cancelled or an unrecoverable bind error occurs.
    pub async fn serve(self) -> Result<(), VerdictError> {
        // Remove stale socket file if present.
        let _ = std::fs::remove_file(&self.path);
        let listener = UnixListener::bind(&self.path)?;
        loop {
            match listener.accept().await {
                Ok((mut stream, _)) => {
                    let json = {
                        let v = self.verdict.read().expect("verdict lock");
                        serde_json::to_vec(&*v).expect("verdict serialisation")
                    };
                    let _ = stream.write_all(&json).await;
                    let _ = stream.write_all(b"\n").await;
                }
                Err(e) => {
                    tracing::warn!("verdict server accept error: {e}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixStream;

    #[test]
    fn default_verdict_is_unverified() {
        let v = TrustVerdict::default();
        assert!(matches!(v, TrustVerdict::Unverified { .. }));
    }

    #[test]
    fn verdict_serialises_tag() {
        let v = TrustVerdict::Verified {
            callsign: "W1AW".into(),
            key_id: "deadbeef".into(),
            last_beacon_utc: 1_746_800_000,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"verdict\":\"verified\""));
        assert!(json.contains("W1AW"));
    }

    #[tokio::test]
    async fn server_writes_verdict_on_connect() {
        let path = std::env::temp_dir().join(format!("openpulse_test_{}.sock", std::process::id()));
        let verdict: SharedVerdict = Arc::new(RwLock::new(TrustVerdict::Verified {
            callsign: "K0ABC".into(),
            key_id: "aabbccdd".into(),
            last_beacon_utc: 1_746_900_000,
        }));
        let server = VerdictServer::new(path.clone(), verdict);
        tokio::spawn(server.serve());
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let mut stream = UnixStream::connect(&path).await.unwrap();
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            stream.read_to_end(&mut buf),
        )
        .await;
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("K0ABC"));
        assert!(text.contains("verified"));
        let _ = std::fs::remove_file(&path);
    }
}
