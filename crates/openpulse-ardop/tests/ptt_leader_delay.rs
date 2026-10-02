//! The PTT leader delay holds the first sample back from the key edge (#1257).
//!
//! Measured at the boundary the defect names: a spy `PttController` stamps the instant it asserts,
//! and a recording output stream stamps the instant the first sample is written. The gap must be at
//! least the configured leader. A lower bound is load-robust (a slow machine only widens it), so this
//! does not inherit #1066's wall-clock trap. The control runs the same path with no leader and bounds
//! the gap from ABOVE, so the leader assertion cannot pass on a build that was already slow.

use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Stamp = Arc<Mutex<Option<Instant>>>;

fn stamp_once(s: &Stamp) {
    let mut g = s.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(Instant::now());
    }
}

fn read(s: &Stamp) -> Option<Instant> {
    *s.lock().unwrap_or_else(|e| e.into_inner())
}

struct SpyPtt {
    asserted_at: Stamp,
    keyed: Arc<AtomicBool>,
}

impl PttController for SpyPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        stamp_once(&self.asserted_at);
        self.keyed.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.keyed.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        self.keyed.load(Ordering::SeqCst)
    }
}

/// An output stream that stamps its first non-empty write; input is silence.
struct RecordingBackend {
    first_write: Stamp,
}

struct RecordingOut {
    first_write: Stamp,
}

impl AudioOutputStream for RecordingOut {
    fn write(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        if !samples.is_empty() {
            stamp_once(&self.first_write);
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

struct SilentIn;

impl AudioInputStream for SilentIn {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        std::thread::sleep(Duration::from_millis(10));
        Ok(vec![0.0; 80])
    }
    fn close(self: Box<Self>) {}
}

impl AudioBackend for RecordingBackend {
    fn name(&self) -> &str {
        "recording"
    }
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        Ok(Vec::new())
    }
    fn open_input(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        Ok(Box::new(SilentIn))
    }
    fn open_output(
        &self,
        _device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        Ok(Box::new(RecordingOut {
            first_write: self.first_write.clone(),
        }))
    }
}

/// Key one data frame through a real ARDOP server built from `ArdopConfig { ptt_leader, .. }` — the
/// path `main.rs` takes from `[modem] ptt_leader_ms` — and return `first_write - ptt_assert`.
fn edge_to_first_sample(leader: Duration) -> Duration {
    let asserted_at: Stamp = Arc::default();
    let first_write: Stamp = Arc::default();
    let mut engine = ModemEngine::new(Box::new(RecordingBackend {
        first_write: first_write.clone(),
    }));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    let server = ArdopServer::with_trust_relay_ptt(
        engine,
        ArdopConfig {
            loopback: false, // the real keyed TX path, not the loopback echo
            ptt_leader: leader,
            ..ArdopConfig::default()
        },
        InMemoryTrustStore::default(),
        None,
        Box::new(SpyPtt {
            asserted_at: asserted_at.clone(),
            keyed: Arc::default(),
        }),
    );
    let bridge = server.bridge();
    *bridge.callsign.try_write().expect("callsign") = "DC0SK".into();
    bridge
        .tx_data_tx
        .send(b"leader".to_vec())
        .expect("queue tx");

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let (Some(a), Some(w)) = (read(&asserted_at), read(&first_write)) {
            return w.saturating_duration_since(a);
        }
        assert!(
            Instant::now() < deadline,
            "the data frame never keyed and wrote"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_first_sample_waits_for_the_leader() {
    let leader = Duration::from_millis(300);
    let gap = edge_to_first_sample(leader);
    assert!(
        gap >= leader,
        "first sample {gap:?} after the PTT edge, under the {leader:?} leader"
    );
}

/// Control: with no leader the same path writes promptly. This is an UPPER bound and deliberately
/// generous — it only has to sit below the 300 ms the test above asserts, so the leader assertion
/// is about the leader and not about a path that was already slow.
#[test]
fn without_a_leader_the_first_sample_follows_the_edge_promptly() {
    let gap = edge_to_first_sample(Duration::ZERO);
    assert!(
        gap < Duration::from_millis(300),
        "first sample {gap:?} after the PTT edge with no leader — the accidental leader alone \
         is as long as the configured one, so the leader test above cannot discriminate"
    );
}
