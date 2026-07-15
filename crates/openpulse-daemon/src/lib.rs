//! NDJSON-over-TCP control server for the OpenPulse daemon.
//!
//! [`ControlServer::spawn`] binds a TCP listener and accepts one or more
//! concurrent client connections.  Each client receives the full unsolicited
//! [`ControlEvent`] stream and may send [`ControlCommand`] lines which are
//! dispatched back to the caller via an `mpsc` channel.
//!
//! Clients that send [`ControlCommand::SubscribeSpectrum`] receive binary
//! spectrum frames interleaved with the NDJSON event stream on the same
//! connection.  See [`protocol::encode_spectrum_frame`] for the wire format.

pub mod audit;
pub mod logbook;
pub mod monitor;
pub mod protocol;
pub mod ptt;

#[cfg(not(target_arch = "wasm32"))]
pub mod filexfer;

/// WebSocket control endpoint — native server builds only.
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;

/// Daemon run loop (extracted from the `openpulse-server` binary) — native only.
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

/// Twin-station rig: two real daemons bridged through a channel — native only.
#[cfg(not(target_arch = "wasm32"))]
pub mod twin;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use openpulse_channel::dsp::PowerSpectrum;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_core::handshake::{
    verify_conack, verify_conreq, ConAck, ConReq, InMemoryTrustStore, TrustStore,
};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_core::relay::RelayForwarder;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_core::sar::{sar_encode, SarReassembler};
use openpulse_core::trust::{
    classify_connection_trust, CertificateSource, PolicyProfile, PublicKeyTrustLevel, SigningMode,
};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_modem::engine::SecureSessionParams;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_modem::ModemEngine;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::frame::{
    decode_unsigned as decode_qsy_frame, encode_unsigned as encode_qsy_frame, QsyFrame,
};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::session::{QsyAction, QsyPolicy, QsySession};
#[cfg(not(target_arch = "wasm32"))]
use openpulse_qsy::ConnectionTrustLevel;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_radio::CatController;
#[cfg(not(target_arch = "wasm32"))]
use openpulse_repeater::CrossBandRepeater;
#[cfg(not(target_arch = "wasm32"))]
use protocol::{
    encode_spectrum_frame, CommandResponse, ControlCommand, ControlEvent, MessageSummary,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::{TcpListener, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

pub use protocol::ControlCommand as Command;
pub use protocol::ControlEvent as Event;

/// Live engine metrics shared between the main loop and the periodic metrics task.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct MetricsSnapshot {
    pub afc_correction_hz: f32,
    /// Cumulative bytes decoded from the RF receive path.
    pub total_rx_bytes: u64,
    /// Smoothed (EWMA) modem receive-path decode latency in milliseconds.
    pub decode_latency_ms: f32,
    /// Cumulative raw bytes of decoded RX payloads measured for compressibility.
    pub raw_payload_bytes: u64,
    /// Cumulative best-effort compressed size of those payloads (the session LZ4/zstd compressor,
    /// including its framing overhead; never larger than raw). The ratio `compressed / raw` is the
    /// live compression figure reported in `ControlEvent::Metrics`.
    pub compressed_payload_bytes: u64,
}

/// Compression ratio (compressed / raw) of the measured payload stream, or `None` before any payload
/// has been seen. Matches the `ControlEvent::Metrics.compress_ratio` convention (compressed / raw).
fn compression_ratio(raw: u64, compressed: u64) -> Option<f32> {
    (raw > 0).then(|| compressed as f32 / raw as f32)
}

#[cfg(not(target_arch = "wasm32"))]
type SharedMetrics = Arc<Mutex<MetricsSnapshot>>;

/// Sample the daemon process's CPU and memory load. Returns
/// `(cpu_percent, ram_mib, ram_percent_of_total)`, where CPU is the conventional process
/// reading (100% = one core fully used; may exceed 100% for a multi-threaded process).
#[cfg(not(target_arch = "wasm32"))]
fn sample_process_resources(sys: &mut sysinfo::System, pid: sysinfo::Pid) -> (f32, f32, f32) {
    sys.refresh_memory();
    sys.refresh_process(pid);
    let total = sys.total_memory().max(1) as f32;
    match sys.process(pid) {
        Some(p) => {
            let rss = p.memory() as f32;
            (
                p.cpu_usage().max(0.0),
                rss / (1024.0 * 1024.0),
                (rss / total * 100.0).clamp(0.0, 100.0),
            )
        }
        None => (0.0, 0.0, 0.0),
    }
}

/// Best-effort system GPU utilisation (0–100). Queries NVIDIA via `nvidia-smi`; on a host with
/// no such tool (or a non-NVIDIA GPU) it marks `available` false so subsequent ticks don't keep
/// spawning a failing process, and returns `None`.
#[cfg(not(target_arch = "wasm32"))]
fn read_gpu_utilization(available: &mut bool) -> Option<f32> {
    if !*available {
        return None;
    }
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<f32>().ok()),
        _ => {
            *available = false;
            None
        }
    }
}

/// Mutable daemon runtime state touched by side-effectful control commands.
#[cfg(not(target_arch = "wasm32"))]
pub struct RuntimeControlState {
    pub repeater_enabled: bool,
    pub qsy_decisions: HashMap<String, bool>,
    pub qsy_pending_token: Option<String>,
    /// Active QSY negotiation session (present after operator accepts a pending token).
    pub qsy_session: Option<QsySession>,
    /// Candidate frequencies (Hz) supplied from config for QSY scanning.
    pub qsy_candidate_freqs: Vec<u64>,
    /// QSY policy parsed from config; governs which requests are accepted.
    pub qsy_policy: QsyPolicy,
    /// Dwell time per frequency during a QSY scan (milliseconds).
    pub qsy_scan_dwell_ms: u64,
    /// Switchover offset (seconds) encoded in outgoing QSY_ACK frames.
    pub qsy_switchover_offset_s: u32,
    /// Pre-built cross-band repeater; taken and moved into a thread by EnableRepeater.
    pub repeater: Option<CrossBandRepeater>,
    /// Stop flag for the running repeater thread.
    pub repeater_stop: Option<Arc<AtomicBool>>,
    /// Handle for the running repeater thread.
    pub repeater_thread: Option<std::thread::JoinHandle<()>>,
    /// PTT hardware + watchdog deadline behind a shared lock, so an independent watchdog thread can
    /// force-release the transmitter even while the async command loop is blocked in a long handler
    /// (issue #863). Keyed ⇔ the deadline is armed; the default max keyed duration is 180 s (Part 97).
    pub ptt: crate::ptt::SharedPtt,
    /// Loaded trust store for verifying incoming peer handshakes.
    pub trust_store: InMemoryTrustStore,
    /// Optional relay forwarder; `Some` when `[relay] enabled = true` in config.
    pub relay_forwarder: Option<RelayForwarder>,
    /// Fallback DCD/squelch RMS threshold when no per-band override matches.
    pub dcd_squelch_default: f32,
    /// Per-band DCD/squelch overrides (band label → threshold), applied on retune.
    pub dcd_squelch_bands: std::collections::BTreeMap<String, f32>,
    /// Global TX attenuation (dB) applied when no per-band override matches the current band.
    pub tx_attenuation_default: f32,
    /// Per-band TX attenuation overrides (band label → dB), set by `SetTxAttenuation { band }` and
    /// re-applied on retune.
    pub tx_attenuation_bands: std::collections::BTreeMap<String, f32>,
    /// Automatic ADIF logbook (opt-in); records one QSO per connect→disconnect.
    pub logbook: crate::logbook::Logbook,
    /// Most recent CAT frequency (Hz) set via `SetFreq`, stamped into the logbook QSO.
    pub last_freq_hz: Option<u64>,
    /// 32-byte Ed25519 seed identifying this station; signs outgoing CONREQ/CONACK frames.
    pub station_seed: [u8; 32],
    /// Local callsign advertised as the handshake `station_id`.
    pub local_callsign: String,
    /// Local Maidenhead grid advertised in the handshake (empty = not advertised).
    pub local_grid: String,
    /// Outstanding CONREQ awaiting a CONACK (initiator role); `None` when idle.
    pub pending_handshake: Option<PendingHandshake>,
    /// Most recently verified peer identity from a completed signed handshake.
    pub verified_peer: Option<VerifiedPeer>,
    /// Reassembles inbound SAR-fragmented handshake frames (CONREQ/CONACK exceed one modem frame).
    pub handshake_sar: SarReassembler,
    /// Our active OTA rate-ladder identity `(profile_name, fingerprint)`, set at OTA startup. Used to
    /// advertise our ladder in the handshake and to detect a diverged peer ladder. `None` = no OTA.
    pub local_ota_ladder: Option<(String, u64)>,
    /// Compress fixed-mode `SendMessage` payloads before transmission (`[compression] enabled`). The OTA
    /// path is packed in `server::run`; this covers the non-OTA transmit inside `apply_command_to_engine`.
    pub compress_tx: bool,
    /// Reassembles inbound `OPFX` file-transfer control frames (segment-id `0xFFFF`); block-data
    /// fragments (segment-id `block_index + 1`) are reassembled inside the active receive session.
    pub filexfer_sar: SarReassembler,
    /// Tripwire: number of inbound `OPFX` frames routed to the file-transfer path. Stays 0 unless a
    /// file frame actually reaches the seam on the production receive path (seam-gap discipline).
    pub filexfer_frames_routed: u64,
    /// Active inbound file-transfer session (at most one per link in v1).
    pub file_rx: Option<crate::filexfer::FxRxState>,
    /// Received files this session, newest last — served by `ListFiles` so a late-connecting client
    /// sees transfers that completed before it attached (not just live `FileReceived` events).
    pub received_files: Vec<crate::protocol::FileSummary>,
    /// Active outbound file-transfer session (at most one per link in v1).
    pub file_tx: Option<crate::filexfer::FxTxState>,
    /// Storage + acceptance policy from `[file_transfer]` config.
    pub filexfer_policy: crate::filexfer::FileTransferPolicy,
    /// Frames the file-transfer sessions want on air, `(sar_fragment, mode)`. `server::run` drains this
    /// with a single PTT keying per burst; queueing (not transmitting inline) keeps the module I/O-free
    /// while the PTT controller — which lives in `server::run` — sequences the half-duplex TX.
    pub filexfer_tx_queue: Vec<(Vec<u8>, String)>,
    /// JS8 station-discovery runtime (FF-15), present when `[discovery]` is configured. `enabled`
    /// gates activity; `server::run` feeds it captured audio + the idle predicate and executes its
    /// retune outcomes. `None` when discovery is not built for this daemon.
    pub discovery: Option<openpulse_discovery::DiscoveryRuntime>,
    /// Simultaneous multi-mode receive (REQ-RX-01); `None` when the monitor is off or has no modes.
    pub monitor: Option<crate::monitor::MonitorRuntime>,
    /// Home frequency (Hz) saved when discovery QSYed to the JS8 calling channel, restored on stand-down;
    /// `None` when not dwelling. The home frequency itself comes from `last_freq_hz`.
    pub discovery_home_freq_hz: Option<u64>,
    /// JS8 calling frequency (Hz) per band label (from `[discovery]` config). Discovery dwells on the
    /// entry for the operator's current home band; empty when discovery is not configured.
    pub discovery_calling_freqs_hz: std::collections::BTreeMap<String, u64>,
    /// Rendezvous working channels (Hz) per band label (from `[discovery]` config). A rendezvous agrees
    /// a channel **index** into the current band's list; the daemon resolves it to Hz for the QSY.
    pub discovery_rendezvous_channels_hz: std::collections::BTreeMap<String, Vec<u64>>,
    /// A scheduled post-rendezvous QSY: `(peer, freq_hz, due_at_ms)`. Set when a rendezvous is agreed; the
    /// QSY + CONREQ handoff fire once the `switch_in_slots` delay elapses (both stations retune together
    /// and the Accept has time to be heard first).
    pub rendezvous_qsy_due: Option<(String, u64, u64)>,
    /// Set by `discovery_tick` the tick a rendezvous QSY completes: `(peer, freq_hz)`. `server::run`
    /// takes it and runs the `ConnectPeer` handshake handoff (needs `&mut engine`), then clears it.
    pub rendezvous_connect_ready: Option<(String, u64)>,
    /// Shared peer cache (§5.2): recognized OpenPulse peers mapped from discovery's hinted stations,
    /// queryable by capability/quality/trust for rendezvous, relay routing, and peer queries.
    pub peer_cache: openpulse_core::peer_cache::PeerCache,
}

impl RuntimeControlState {
    /// True when a verified peer's OTA ladder is known to differ from ours — OTA rate-stepping must
    /// be suppressed (fixed-mode fallback) so a `recommended_level` can't mean different modes.
    /// OTA without a handshake, or with a compatible/undetermined peer, is unaffected.
    pub fn ota_suppressed_by_peer(&self) -> bool {
        matches!(&self.verified_peer, Some(p) if p.profile_compatible == Some(false))
    }

    /// True when the station has a real callsign to transmit under. §97.119 forbids keying the
    /// transmitter without a station ID, and periodic auto-ID is disabled for an empty/`N0CALL`
    /// callsign — so an *autonomous* responder (CONACK, relay, QSY) that merely heard a frame must
    /// not key up at all without a valid MYID, or it would transmit unidentified.
    pub fn local_callsign_valid(&self) -> bool {
        let c = self.local_callsign.trim();
        !c.is_empty() && !c.eq_ignore_ascii_case("N0CALL")
    }

    /// Over-air trust level of the last peer we verified this session, for gating the (unauthenticated)
    /// QSY responder. RF certificates are `OverAir` without PSK, so this tops out at `Reduced` (a
    /// trust-store key) or `Low` (first-seen) — never `Verified`, which requires an out-of-band cert.
    /// Best-effort: a single global `verified_peer` slot is not bound to the QSY requester (the QSY
    /// frame carries no signature), so `allow_trustlevels` means "only after such a handshake this
    /// session," not a per-requester check.
    pub fn rf_peer_trust(&self) -> ConnectionTrustLevel {
        match &self.verified_peer {
            Some(p) => {
                let key_trust = self.trust_store.trust_level(&p.callsign);
                classify_connection_trust(key_trust, CertificateSource::OverAir, false).decision
            }
            None => ConnectionTrustLevel::Unverified,
        }
    }
}

/// An in-flight CONREQ the daemon sent and is awaiting a CONACK for.
#[derive(Clone, Debug)]
pub struct PendingHandshake {
    /// Session id echoed by the peer's CONACK.
    pub session_id: String,
    /// Callsign the operator asked to connect to.
    pub peer_callsign: String,
    /// When the CONREQ went out, for timeout expiry.
    pub started_at: Instant,
}

/// A peer identity proven by a verified Ed25519 handshake signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedPeer {
    /// Peer station id (callsign) from the signed frame.
    pub callsign: String,
    /// Peer Maidenhead grid from the signed frame (empty = not advertised).
    pub grid: String,
    /// Peer Ed25519 verifying-key bytes.
    pub pubkey: Vec<u8>,
    /// Whether the peer's advertised OTA rate ladder matches ours: `Some(true)` = compatible,
    /// `Some(false)` = the ladders diverged (OTA adaptation is unsafe → suppressed), `None` = not
    /// determinable (we or the peer advertised no OTA ladder). See `docs/dev/design/ladder-versioning.md`.
    pub profile_compatible: Option<bool>,
}

/// Timeout after which an unanswered CONREQ is abandoned.
#[cfg(not(target_arch = "wasm32"))]
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Reassembly timeout for inbound file-transfer control fragments.
#[cfg(not(target_arch = "wasm32"))]
pub const FILEXFER_SAR_TIMEOUT: Duration = Duration::from_secs(300);

#[cfg(not(target_arch = "wasm32"))]
impl Default for RuntimeControlState {
    fn default() -> Self {
        Self {
            repeater_enabled: false,
            qsy_decisions: HashMap::new(),
            qsy_pending_token: None,
            qsy_session: None,
            qsy_candidate_freqs: Vec::new(),
            qsy_policy: QsyPolicy::default(),
            qsy_scan_dwell_ms: 500,
            qsy_switchover_offset_s: 5,
            repeater: None,
            repeater_stop: None,
            repeater_thread: None,
            ptt: crate::ptt::SharedPtt::default(),
            trust_store: InMemoryTrustStore::default(),
            relay_forwarder: None,
            dcd_squelch_default: 0.01,
            dcd_squelch_bands: std::collections::BTreeMap::new(),
            tx_attenuation_default: 0.0,
            tx_attenuation_bands: std::collections::BTreeMap::new(),
            logbook: crate::logbook::Logbook::default(),
            last_freq_hz: None,
            station_seed: [0u8; 32],
            local_callsign: String::new(),
            local_grid: String::new(),
            pending_handshake: None,
            verified_peer: None,
            handshake_sar: SarReassembler::new(HANDSHAKE_TIMEOUT),
            local_ota_ladder: None,
            compress_tx: false,
            filexfer_sar: SarReassembler::new(FILEXFER_SAR_TIMEOUT),
            filexfer_frames_routed: 0,
            file_rx: None,
            received_files: Vec::new(),
            file_tx: None,
            filexfer_policy: crate::filexfer::FileTransferPolicy::default(),
            filexfer_tx_queue: Vec::new(),
            discovery: None,
            monitor: None,
            discovery_home_freq_hz: None,
            discovery_calling_freqs_hz: std::collections::BTreeMap::new(),
            discovery_rendezvous_channels_hz: std::collections::BTreeMap::new(),
            rendezvous_qsy_due: None,
            rendezvous_connect_ready: None,
            peer_cache: openpulse_core::peer_cache::PeerCache::new(256, 3_600_000),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for RuntimeControlState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeControlState")
            .field("repeater_enabled", &self.repeater_enabled)
            .field("qsy_decisions", &self.qsy_decisions)
            .field("qsy_pending_token", &self.qsy_pending_token)
            .field("qsy_session", &self.qsy_session.is_some())
            .field("qsy_candidate_freqs", &self.qsy_candidate_freqs)
            .field("qsy_switchover_offset_s", &self.qsy_switchover_offset_s)
            .field("repeater", &self.repeater.is_some())
            .field("repeater_stop", &self.repeater_stop.is_some())
            .field("repeater_thread", &self.repeater_thread.is_some())
            .field("ptt", &self.ptt)
            .field("trust_store_entries", &"<opaque>")
            .field("relay_forwarder", &self.relay_forwarder.is_some())
            .finish()
    }
}

/// Shared mutable mode string, written by `set_mode` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedMode = Arc<Mutex<String>>;
/// Read-only set of mode names the engine's registered plugins support, captured at startup so the
/// control-command dispatcher can reject an unknown `SetMode`/`SetConfig` *before* writing shared state.
#[cfg(not(target_arch = "wasm32"))]
pub type ValidModes = Arc<std::collections::HashSet<String>>;
/// Shared mutable TX attenuation (dB), written by `set_tx_attenuation` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedAttenuation = Arc<Mutex<f32>>;
/// Shared QSY enabled flag, toggled by `set_config` commands.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedQsyEnabled = Arc<Mutex<bool>>;
/// Shared bandplan mode string (`"unrestricted"`, `"ham-iaru-r1"`, etc.).
#[cfg(not(target_arch = "wasm32"))]
pub type SharedBandplanMode = Arc<Mutex<String>>;
/// Shared flag: allow integrated tuner operation when SWR is high.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedTunerOnHighSWR = Arc<Mutex<bool>>;
/// Shared audio sample tap for spectrum computation (most-recent 1024 samples).
#[cfg(not(target_arch = "wasm32"))]
pub type SpectrumTap = Arc<RwLock<Vec<f32>>>;
/// Shared station identity strings (callsign + grid square), set at startup.
#[cfg(not(target_arch = "wasm32"))]
pub type SharedStationId = Arc<Mutex<(String, String)>>;
/// Shared in-memory message store (sent and received messages).
#[cfg(not(target_arch = "wasm32"))]
pub type SharedMessageStore = Arc<Mutex<MessageStore>>;

/// Initial state used when starting the TCP control server.
#[cfg(not(target_arch = "wasm32"))]
pub struct ControlServerConfig {
    pub initial_mode: String,
    pub initial_station_id: (String, String),
    pub initial_qsy_enabled: bool,
    pub initial_bandplan_mode: String,
    pub initial_allow_tuner_on_high_swr: bool,
    /// Control-channel PSK: `Some` requires each client to complete a Noise handshake
    /// (REQ-SEC-CTL-01/02); `None` runs the plaintext path (loopback default).
    pub control_psk: Option<[u8; openpulse_linksec::PSK_LEN]>,
}

/// Maximum number of messages kept in memory; oldest are evicted when full.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_MESSAGES: usize = 500;

/// A single stored message (sent or received).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub timestamp_secs: u64,
}

/// In-memory inbox with a monotonically increasing ID counter.
#[cfg(not(target_arch = "wasm32"))]
pub struct MessageStore {
    next_id: u64,
    pub messages: std::collections::VecDeque<StoredMessage>,
}

#[cfg(not(target_arch = "wasm32"))]
impl MessageStore {
    fn new() -> Self {
        Self {
            next_id: 1,
            messages: std::collections::VecDeque::new(),
        }
    }

    /// Allocate the next unique message ID.
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// All shared state passed to each per-client TCP handler.
#[cfg(not(target_arch = "wasm32"))]
struct ClientCtx {
    ev_tx: Arc<broadcast::Sender<ControlEvent>>,
    cmd_tx: mpsc::Sender<ControlCommand>,
    active_mode: SharedMode,
    tx_attenuation_db: SharedAttenuation,
    qsy_enabled: SharedQsyEnabled,
    bandplan_mode: SharedBandplanMode,
    allow_tuner_on_high_swr: SharedTunerOnHighSWR,
    spectrum_tap: SpectrumTap,
    station_id: SharedStationId,
    message_store: SharedMessageStore,
    valid_modes: ValidModes,
}

/// Handle returned by [`ControlServer::spawn`].
///
/// Dropping this handle does *not* stop the server — use [`ControlServerHandle::shutdown`]
/// for a clean stop (or just let the process exit).
#[cfg(not(target_arch = "wasm32"))]
pub struct ControlServerHandle {
    /// Receives every [`ControlCommand`] dispatched from any connected client.
    pub commands: mpsc::Receiver<ControlCommand>,
    /// Sender for the shared event broadcast (pass to [`ws::spawn_ws`] to
    /// share state between the TCP and WebSocket control endpoints).
    pub event_tx: Arc<broadcast::Sender<ControlEvent>>,
    /// mpsc sender for injecting commands programmatically (used by WebSocket endpoint).
    pub command_tx: mpsc::Sender<ControlCommand>,
    /// Current active mode string (also updated by the command handler).
    pub active_mode: SharedMode,
    /// Current TX attenuation in dB (also updated by the command handler).
    pub tx_attenuation_db: SharedAttenuation,
    /// Whether QSY frequency-agility is enabled.
    pub qsy_enabled: SharedQsyEnabled,
    /// Active bandplan guardrail mode string.
    pub bandplan_mode: SharedBandplanMode,
    /// Whether tuner-on-high-SWR behavior is allowed.
    pub allow_tuner_on_high_swr: SharedTunerOnHighSWR,
    /// Audio sample tap; caller may write recent RX samples here.
    pub spectrum_tap: SpectrumTap,
    /// Station callsign and grid square loaded from config at startup.
    pub station_id: SharedStationId,
    /// In-memory message store shared across all control endpoints.
    pub message_store: SharedMessageStore,
    /// Live engine metrics written by the main loop; read by the periodic metrics task.
    pub shared_metrics: SharedMetrics,
    /// Mode names the registered plugins support (captured at startup), for pre-write validation of
    /// `SetMode`/`SetConfig` on both the TCP and WebSocket dispatch paths.
    pub valid_modes: ValidModes,
}

/// NDJSON-over-TCP control server.
#[cfg(not(target_arch = "wasm32"))]
pub struct ControlServer;

#[cfg(not(target_arch = "wasm32"))]
impl ControlServer {
    /// Spawn the control server on `addr`.
    ///
    /// `engine` is used to subscribe to the event broadcast channel.
    /// The bound address is written to `bound_addr` if provided (useful in
    /// tests that bind on port 0 and need the ephemeral port).
    pub async fn spawn(
        addr: SocketAddr,
        engine: &ModemEngine,
        config: ControlServerConfig,
        bound_addr: Option<&mut SocketAddr>,
    ) -> Result<ControlServerHandle, std::io::Error> {
        let listener = TcpListener::bind(addr).await?;
        if let Some(out) = bound_addr {
            *out = listener.local_addr()?;
        }

        let (ev_tx, _) = broadcast::channel::<ControlEvent>(256);
        let ev_tx = Arc::new(ev_tx);
        let (cmd_tx, cmd_rx) = mpsc::channel::<ControlCommand>(64);

        let active_mode = Arc::new(Mutex::new(config.initial_mode));
        let tx_attenuation_db: SharedAttenuation = Arc::new(Mutex::new(0.0f32));
        let qsy_enabled: SharedQsyEnabled = Arc::new(Mutex::new(config.initial_qsy_enabled));
        let bandplan_mode: SharedBandplanMode = Arc::new(Mutex::new(config.initial_bandplan_mode));
        let allow_tuner_on_high_swr: SharedTunerOnHighSWR =
            Arc::new(Mutex::new(config.initial_allow_tuner_on_high_swr));
        let spectrum_tap: SpectrumTap = Arc::new(RwLock::new(vec![0.0f32; 1024]));
        let station_id: SharedStationId = Arc::new(Mutex::new(config.initial_station_id));
        let message_store: SharedMessageStore = Arc::new(Mutex::new(MessageStore::new()));
        let shared_metrics: SharedMetrics = Arc::new(Mutex::new(MetricsSnapshot::default()));
        // Capture the registered mode names once, so a bad SetMode is rejected before it mutates state.
        let valid_modes: ValidModes = Arc::new(
            engine
                .plugins()
                .list()
                .iter()
                .flat_map(|info| info.supported_modes.iter().cloned())
                .collect(),
        );

        // Background task: forward EngineEvents into the ControlEvent broadcast.
        let mut eng_rx = engine.subscribe();
        let ev_fwd = Arc::clone(&ev_tx);
        tokio::spawn(async move {
            loop {
                match eng_rx.recv().await {
                    Ok(ev) => {
                        let _ = ev_fwd.send(ControlEvent::EngineEvent { event: ev });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "engine event receiver lagged; events dropped");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // Background task: periodic Metrics + SystemMetrics events at 1 Hz.
        let ev_metrics = Arc::clone(&ev_tx);
        let metrics_snap = Arc::clone(&shared_metrics);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            let mut last_bytes: u64 = 0;
            // Host-resource sampling state (daemon process).
            let mut sys = sysinfo::System::new();
            let pid = sysinfo::Pid::from_u32(std::process::id());
            // Prime the CPU baseline so the first emit reports a real delta, not 0.
            sys.refresh_process(pid);
            let mut gpu_available = true;
            // Our-kernel GPU-busy tracking (only meaningful with the `gpu` feature).
            #[cfg(feature = "gpu")]
            let mut last_gpu_busy = openpulse_gpu::gpu_busy_nanos();
            #[cfg(feature = "gpu")]
            let mut last_gpu_instant = std::time::Instant::now();
            loop {
                interval.tick().await;
                let (afc, new_bytes, decode_latency_ms, raw_bytes, compressed_bytes) = {
                    let m = metrics_snap.lock().await;
                    (
                        m.afc_correction_hz,
                        m.total_rx_bytes,
                        m.decode_latency_ms,
                        m.raw_payload_bytes,
                        m.compressed_payload_bytes,
                    )
                };
                let effective_bps = (new_bytes.saturating_sub(last_bytes) * 8) as f32;
                last_bytes = new_bytes;
                let _ = ev_metrics.send(ControlEvent::Metrics {
                    effective_bps,
                    ecc_rate: None,
                    compress_ratio: compression_ratio(raw_bytes, compressed_bytes),
                    afc_correction_hz: afc,
                    signal_strength_dbm: None,
                });

                let (cpu_percent, ram_mb, ram_percent) = sample_process_resources(&mut sys, pid);

                // GPU load: prefer the time our wgpu kernels actually spent on the GPU this
                // interval; fall back to a best-effort system source when the gpu feature is
                // off or no kernels ran (CPU path / no adapter).
                #[cfg(feature = "gpu")]
                let gpu_percent = {
                    let now_busy = openpulse_gpu::gpu_busy_nanos();
                    let now = std::time::Instant::now();
                    let busy = now_busy.saturating_sub(last_gpu_busy) as f64;
                    let elapsed = now.duration_since(last_gpu_instant).as_nanos() as f64;
                    last_gpu_busy = now_busy;
                    last_gpu_instant = now;
                    let kernel_pct = if elapsed > 0.0 {
                        (busy / elapsed * 100.0).clamp(0.0, 100.0) as f32
                    } else {
                        0.0
                    };
                    if kernel_pct > 0.05 {
                        Some(kernel_pct)
                    } else {
                        tokio::task::block_in_place(|| read_gpu_utilization(&mut gpu_available))
                    }
                };
                #[cfg(not(feature = "gpu"))]
                let gpu_percent =
                    tokio::task::block_in_place(|| read_gpu_utilization(&mut gpu_available));
                let _ = ev_metrics.send(ControlEvent::SystemMetrics {
                    cpu_percent,
                    ram_mb,
                    ram_percent,
                    gpu_percent,
                    decode_latency_ms,
                });
            }
        });

        // Acceptor task.
        let ev_tx_a = Arc::clone(&ev_tx);
        let cmd_tx_a = cmd_tx.clone();
        let mode_a = Arc::clone(&active_mode);
        let atten_a = Arc::clone(&tx_attenuation_db);
        let qsy_a = Arc::clone(&qsy_enabled);
        let bp_a = Arc::clone(&bandplan_mode);
        let tuner_a = Arc::clone(&allow_tuner_on_high_swr);
        let tap_a = Arc::clone(&spectrum_tap);
        let sid_a = Arc::clone(&station_id);
        let store_a = Arc::clone(&message_store);
        let modes_a = Arc::clone(&valid_modes);
        let control_psk = config.control_psk;
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        tracing::info!(%peer, "control port: client connected");
                        let ctx = ClientCtx {
                            ev_tx: Arc::clone(&ev_tx_a),
                            cmd_tx: cmd_tx_a.clone(),
                            active_mode: Arc::clone(&mode_a),
                            tx_attenuation_db: Arc::clone(&atten_a),
                            qsy_enabled: Arc::clone(&qsy_a),
                            bandplan_mode: Arc::clone(&bp_a),
                            allow_tuner_on_high_swr: Arc::clone(&tuner_a),
                            spectrum_tap: Arc::clone(&tap_a),
                            station_id: Arc::clone(&sid_a),
                            message_store: Arc::clone(&store_a),
                            valid_modes: Arc::clone(&modes_a),
                        };
                        let rx = ev_tx_a.subscribe();
                        tokio::spawn(handle_client(stream, rx, ctx, control_psk));
                    }
                    Err(e) => tracing::warn!("control port accept error: {e}"),
                }
            }
        });

        Ok(ControlServerHandle {
            commands: cmd_rx,
            event_tx: ev_tx,
            command_tx: cmd_tx,
            active_mode,
            tx_attenuation_db,
            qsy_enabled,
            bandplan_mode,
            allow_tuner_on_high_swr,
            spectrum_tap,
            station_id,
            message_store,
            shared_metrics,
            valid_modes,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Mode-aware control-connection writer: plaintext (loopback) or a PSK-authenticated Noise channel.
enum ClientWriter {
    Plain(tokio::net::tcp::OwnedWriteHalf),
    Noise(openpulse_linksec::async_channel::NoiseWriteHalf<tokio::io::WriteHalf<TcpStream>>),
}

impl ClientWriter {
    /// Write one JSON value as a protocol message (a `\n`-terminated line on the plaintext path;
    /// a length-framed Noise message on the authenticated path).
    async fn write_json<T: serde::Serialize>(&mut self, v: &T) -> Result<(), ()> {
        let s = serde_json::to_string(v).map_err(|_| ())?;
        match self {
            ClientWriter::Plain(w) => {
                let mut line = s;
                line.push('\n');
                w.write_all(line.as_bytes()).await.map_err(|_| ())
            }
            ClientWriter::Noise(w) => w.send(s.as_bytes()).await.map_err(|_| ()),
        }
    }

    /// Write one raw binary frame (e.g. a spectrum frame).
    async fn write_frame(&mut self, bytes: &[u8]) -> Result<(), ()> {
        match self {
            ClientWriter::Plain(w) => w.write_all(bytes).await.map_err(|_| ()),
            ClientWriter::Noise(w) => w.send(bytes).await.map_err(|_| ()),
        }
    }
}

/// Mode-aware control-connection reader yielding one NDJSON command per message.
enum ClientReader {
    Plain(tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>),
    Noise(openpulse_linksec::async_channel::NoiseReadHalf<tokio::io::ReadHalf<TcpStream>>),
}

impl ClientReader {
    /// Next command line: `Ok(Some(line))`, `Ok(None)` on clean close, `Err(())` on error.
    async fn next_command(&mut self) -> Result<Option<String>, ()> {
        match self {
            ClientReader::Plain(l) => l.next_line().await.map_err(|_| ()),
            ClientReader::Noise(r) => match r.recv().await {
                Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| ()),
                Err(_) => Ok(None),
            },
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    mut ev_rx: broadcast::Receiver<ControlEvent>,
    ctx: ClientCtx,
    control_psk: Option<[u8; openpulse_linksec::PSK_LEN]>,
) {
    // When a PSK is configured (non-loopback bind or require_auth), the client must complete the
    // Noise handshake; a wrong/absent PSK drops the connection before any command is processed
    // (fail closed, REQ-SEC-CTL-02). Otherwise the channel is plaintext (loopback default).
    let (mut reader, mut write_half) = match control_psk {
        Some(psk) => {
            match openpulse_linksec::async_channel::AsyncNoise::responder(stream, &psk).await {
                Ok(ch) => {
                    let (w, r) = ch.into_split();
                    (ClientReader::Noise(r), ClientWriter::Noise(w))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "control client failed the PSK handshake; dropping (fail closed)");
                    return;
                }
            }
        }
        None => {
            let (read_half, write_half) = stream.into_split();
            (
                ClientReader::Plain(BufReader::new(read_half).lines()),
                ClientWriter::Plain(write_half),
            )
        }
    };

    let (spec_frame_tx, mut spec_frame_rx) = mpsc::channel::<Vec<u8>>(4);
    let mut spectrum_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        tokio::select! {
            Some(frame) = spec_frame_rx.recv() => {
                if write_half.write_frame(&frame).await.is_err() { break; }
            }
            result = ev_rx.recv() => {
                match result {
                    Ok(ev) => {
                        if write_half.write_json(&ev).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "TCP client event receiver lagged; events dropped");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            result = reader.next_command() => {
                match result {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        let cmd: ControlCommand = match serde_json::from_str(line.trim()) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp = CommandResponse::err(format!("parse error: {e}"));
                                let _ = send_json(&mut write_half, &resp).await;
                                continue;
                            }
                        };
                        if handle_command(cmd, &mut write_half, &spec_frame_tx, &mut spectrum_task, &ctx).await {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Ok(Some(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }

    if let Some(h) = spectrum_task {
        h.abort();
    }
}

/// Dispatch one command; returns `true` when the write failed and the loop should exit.
#[cfg(not(target_arch = "wasm32"))]
// TCP control-port command handler. The request-response commands below (those that return data,
// not just an ok) are handled inline; everything else falls to `dispatch_command`. The WebSocket
// path in `ws.rs` mirrors this exact inline set — KEEP THE TWO IN SYNC: a request-response command
// added here but not in `ws.rs` (or vice versa) silently falls to `dispatch_command` on the other
// transport and returns no data. (Audited 2026-06-27: both transports are at parity.)
async fn handle_command(
    cmd: ControlCommand,
    write_half: &mut ClientWriter,
    spec_frame_tx: &mpsc::Sender<Vec<u8>>,
    spectrum_task: &mut Option<tokio::task::JoinHandle<()>>,
    ctx: &ClientCtx,
) -> bool {
    match &cmd {
        ControlCommand::SubscribeSpectrum { fps } => {
            let fps = (*fps).clamp(1, 100);
            if let Some(h) = spectrum_task.take() {
                h.abort();
            }
            let tap = Arc::clone(&ctx.spectrum_tap);
            let tx = spec_frame_tx.clone();
            let period = Duration::from_millis(1000 / fps as u64);
            *spectrum_task = Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(period);
                let mut ps = PowerSpectrum::new();
                loop {
                    interval.tick().await;
                    let bins = ps.compute(&tap.read().await);
                    let frame = encode_spectrum_frame(8000, &bins);
                    if tx.send(frame).await.is_err() {
                        break;
                    }
                }
            }));
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::GetConfig => {
            let (cs, gs) = ctx.station_id.lock().await.clone();
            // Hold all locks simultaneously so the snapshot is consistent with SetConfig.
            let mode_guard = ctx.active_mode.lock().await;
            let atten_guard = ctx.tx_attenuation_db.lock().await;
            let qsy_guard = ctx.qsy_enabled.lock().await;
            let bp_guard = ctx.bandplan_mode.lock().await;
            let tuner_guard = ctx.allow_tuner_on_high_swr.lock().await;
            let config = protocol::DaemonConfig {
                callsign: cs,
                grid_square: gs,
                mode: mode_guard.clone(),
                tx_attenuation_db: *atten_guard,
                qsy_enabled: *qsy_guard,
                bandplan_mode: bp_guard.clone(),
                allow_tuner_on_high_swr: *tuner_guard,
            };
            drop(mode_guard);
            drop(atten_guard);
            drop(qsy_guard);
            drop(bp_guard);
            drop(tuner_guard);
            if send_json(write_half, &ControlEvent::ConfigData { config })
                .await
                .is_err()
            {
                return true;
            }
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::ListMessages => {
            let messages: Vec<MessageSummary> = ctx
                .message_store
                .lock()
                .await
                .messages
                .iter()
                .map(|m| MessageSummary {
                    id: m.id,
                    from: m.from.clone(),
                    to: m.to.clone(),
                    subject: m.subject.clone(),
                    timestamp_secs: m.timestamp_secs,
                })
                .collect();
            if send_json(write_half, &ControlEvent::MessageList { messages })
                .await
                .is_err()
            {
                return true;
            }
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::GetMessage { id } => {
            let found = ctx
                .message_store
                .lock()
                .await
                .messages
                .iter()
                .find(|m| m.id == *id)
                .cloned();
            match found {
                None => send_json(
                    write_half,
                    &CommandResponse::err(format!("unknown id {id}")),
                )
                .await
                .is_err(),
                Some(m) => {
                    let ev = ControlEvent::MessageData {
                        id: m.id,
                        from: m.from,
                        to: m.to,
                        subject: m.subject,
                        body: m.body,
                    };
                    if send_json(write_half, &ev).await.is_err() {
                        return true;
                    }
                    send_json(write_half, &CommandResponse::ok()).await.is_err()
                }
            }
        }

        ControlCommand::SendMessage { to, subject, body } => {
            let from = ctx.station_id.lock().await.0.clone();
            let timestamp_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let id = {
                let mut store = ctx.message_store.lock().await;
                let id = store.alloc_id();
                store.messages.push_back(StoredMessage {
                    id,
                    from: from.clone(),
                    to: to.clone(),
                    subject: subject.clone(),
                    body: body.clone(),
                    timestamp_secs,
                });
                if store.messages.len() > MAX_MESSAGES {
                    let _ = store.messages.pop_front();
                }
                id
            };
            let preview: String = body.chars().take(120).collect();
            let ev = ControlEvent::MessageReceived {
                id,
                from,
                to: to.clone(),
                subject: subject.clone(),
                preview,
                timestamp_secs,
            };
            // Broadcast to all connected clients.
            let _ = ctx.ev_tx.send(ev);
            // Forward to daemon main for RF dispatch.
            let _ = ctx.cmd_tx.send(cmd.clone()).await;
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        ControlCommand::DeleteMessage { id } => {
            ctx.message_store
                .lock()
                .await
                .messages
                .retain(|m| m.id != *id);
            send_json(write_half, &CommandResponse::ok()).await.is_err()
        }

        _ => {
            let resp = dispatch_command(
                &cmd,
                &ctx.cmd_tx,
                &ctx.active_mode,
                &ctx.tx_attenuation_db,
                &ctx.qsy_enabled,
                &ctx.bandplan_mode,
                &ctx.allow_tuner_on_high_swr,
                &ctx.valid_modes,
            )
            .await;
            send_json(write_half, &resp).await.is_err()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn send_json<T: serde::Serialize>(writer: &mut ClientWriter, value: &T) -> Result<(), ()> {
    writer.write_json(value).await
}

/// Apply state-mutating commands and forward all commands to the caller.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_command(
    cmd: &ControlCommand,
    cmd_tx: &mpsc::Sender<ControlCommand>,
    active_mode: &SharedMode,
    tx_attenuation_db: &SharedAttenuation,
    qsy_enabled: &SharedQsyEnabled,
    bandplan_mode: &SharedBandplanMode,
    allow_tuner_on_high_swr: &SharedTunerOnHighSWR,
    valid_modes: &ValidModes,
) -> CommandResponse {
    // Reject an unknown mode BEFORE writing shared state, so a typo can't silently deafen RX +
    // station-ID while the client is told "ok" (the later engine-side validation only logs). An empty
    // set means the caller supplied no registry (tests) — skip validation to preserve their behaviour.
    let requested_mode = match cmd {
        ControlCommand::SetMode { mode } => Some(mode),
        ControlCommand::SetConfig { config } => Some(&config.mode),
        _ => None,
    };
    if let Some(mode) = requested_mode {
        if !valid_modes.is_empty() && !valid_modes.contains(mode) {
            return CommandResponse::err(format!("unsupported mode '{mode}'"));
        }
    }
    if let ControlCommand::SetMode { ref mode } = cmd {
        *active_mode.lock().await = mode.clone();
    }
    if let ControlCommand::SetTxAttenuation { db, band } = cmd {
        // The shared value is the reported global default; a per-band override does not change it (its
        // effect is tracked engine-side and applied on the matching band). See apply_command_to_engine.
        if band.is_none() {
            *tx_attenuation_db.lock().await = *db;
        }
    }
    if let ControlCommand::SetConfig { ref config } = cmd {
        // Hold all locks simultaneously so GetConfig cannot observe a mixed state.
        let (new_qsy, new_bp, new_allow_tuner) = {
            let mut mode = active_mode.lock().await;
            let mut atten = tx_attenuation_db.lock().await;
            let mut qsy = qsy_enabled.lock().await;
            let mut bp = bandplan_mode.lock().await;
            let mut tuner = allow_tuner_on_high_swr.lock().await;
            *mode = config.mode.clone();
            *atten = config.tx_attenuation_db;
            *qsy = config.qsy_enabled;
            *bp = config.bandplan_mode.clone();
            *tuner = config.allow_tuner_on_high_swr;
            (*qsy, bp.clone(), *tuner)
        };
        // Persist QSY settings so they survive a daemon restart.
        if let Err(e) = openpulse_config::save_qsy_config(new_qsy, &new_bp, new_allow_tuner) {
            tracing::warn!("could not persist QSY config: {e}");
        }
    }

    if cmd_tx.send(cmd.clone()).await.is_err() {
        return CommandResponse::err("server shutting down");
    }

    CommandResponse::ok()
}

/// Dispatch a list of [`QsyAction`]s produced by a [`QsySession`].
///
/// Used by both the initiator (`accept_qsy`) and responder (`process_received_bytes`) paths.
#[cfg(not(target_arch = "wasm32"))]
async fn execute_qsy_actions(
    actions: Vec<QsyAction>,
    session: &mut QsySession,
    engine: &mut ModemEngine,
    mut rig_controller: Option<&mut (dyn CatController + Send)>,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    scan_dwell_ms: u64,
) {
    let mut scan_freqs: Option<Vec<u64>> = None;

    for action in actions {
        match action {
            QsyAction::SendFrame(ref frame) => {
                let line = encode_qsy_frame(frame);
                if let Err(e) = engine.transmit(line.as_bytes(), mode, None) {
                    tracing::warn!(error = %e, "qsy: frame transmit failed");
                }
            }
            QsyAction::StartScan { candidates } => {
                scan_freqs = Some(candidates);
            }
            QsyAction::QsyNow { freq_hz } => {
                if let Some(ref mut rig) = rig_controller {
                    if let Err(e) = rig.set_frequency(freq_hz) {
                        tracing::warn!(freq_hz, error = %e, "qsy: set_frequency failed");
                    }
                }
            }
            QsyAction::Reject { reason } => {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "qsy".to_string(),
                    reason: format!("QSY rejected: {reason}"),
                });
            }
        }
    }

    if let Some(freqs) = scan_freqs {
        let results: Vec<(u64, f32)> = if let Some(ref mut rig) = rig_controller {
            // Hop to each candidate, dwell briefly, and read the measured SNR.
            // Save and restore the original frequency so the radio is left on-channel.
            // Rig calls are synchronous TCP I/O — run them in block_in_place so they
            // don't stall the Tokio runtime during the scan.
            let original_freq = match tokio::task::block_in_place(|| rig.get_frequency()) {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "qsy scan: failed to read current frequency; will not restore after scan"
                    );
                    None
                }
            };
            let mut scan_results = Vec::with_capacity(freqs.len());
            for &freq in &freqs {
                if let Err(e) = tokio::task::block_in_place(|| rig.set_frequency(freq)) {
                    tracing::warn!(freq, error = %e, "qsy scan: set_frequency failed; using last SNR");
                    scan_results.push((freq, engine.last_rx_snr_db().unwrap_or(0.0)));
                    continue;
                }
                // Dwell per config to let the audio buffer refresh before sampling SNR.
                tokio::time::sleep(Duration::from_millis(scan_dwell_ms)).await;
                match tokio::task::block_in_place(|| engine.receive(mode, None)) {
                    Ok(_) => {}
                    Err(e) => tracing::warn!(freq, error = %e, "qsy scan: receive failed"),
                }
                scan_results.push((freq, engine.last_rx_snr_db().unwrap_or(0.0)));
            }
            if let Some(orig) = original_freq {
                if let Err(e) = tokio::task::block_in_place(|| rig.set_frequency(orig)) {
                    tracing::warn!(freq = orig, error = %e, "qsy scan: failed to restore frequency");
                }
            }
            scan_results
        } else {
            // No rig controller: fall back to uniform SNR from the most recent receive.
            let observed_snr = engine.last_rx_snr_db().unwrap_or(0.0);
            freqs.iter().map(|&f| (f, observed_snr)).collect()
        };
        match session.scan_complete(results) {
            Ok(follow_up) => {
                // scan_complete never returns another StartScan; iterate directly.
                for action in follow_up {
                    match action {
                        QsyAction::SendFrame(ref frame) => {
                            let line = encode_qsy_frame(frame);
                            if let Err(e) = engine.transmit(line.as_bytes(), mode, None) {
                                tracing::warn!(error = %e, "qsy: post-scan frame transmit failed");
                            }
                        }
                        QsyAction::QsyNow { freq_hz } => {
                            if let Some(ref mut rig) = rig_controller {
                                if let Err(e) = rig.set_frequency(freq_hz) {
                                    tracing::warn!(freq_hz, error = %e, "qsy: post-scan set_frequency failed");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "qsy: scan_complete failed"),
        }
    }
}

/// Release PTT if the watchdog deadline has elapsed since `PttAssert`.
///
/// Returns `true` if the watchdog fired (PTT was forcibly released). The hardware release now happens
/// inside [`ptt::SharedPtt::force_release_if_expired`], so callers no longer need to propagate it —
/// this is a thin delegate kept for the async command loop's cooperative poll. The independent
/// watchdog thread ([`ptt::SharedPtt::spawn_watchdog`]) fires the same path when the loop is blocked.
#[cfg(not(target_arch = "wasm32"))]
pub fn check_ptt_watchdog(
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) -> bool {
    runtime_state.ptt.force_release_if_expired(event_tx)
}

/// Process raw bytes received from the modem engine and drive QSY responder logic.
///
/// Called from the main daemon loop after each receive tick. Non-QSY payloads are
/// silently discarded; only valid [`QsyFrame`] lines advance the session.
#[cfg(not(target_arch = "wasm32"))]
pub async fn process_received_bytes(
    bytes: &[u8],
    runtime_state: &mut RuntimeControlState,
    rig_controller: Option<&mut (dyn CatController + Send)>,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    active_mode: &SharedMode,
    engine: &mut ModemEngine,
) {
    if bytes.is_empty() {
        return;
    }
    let mode = active_mode.lock().await.clone();

    // Attempt relay forwarding on the raw bytes before QSY parsing: WireEnvelope frames
    // are binary and would be dropped by the UTF-8 early return below.
    maybe_relay_forward(bytes, &mode, runtime_state, engine);

    // QSY frames are ASCII; a non-QSY, non-relay binary frame is a candidate handshake SAR
    // fragment (CONREQ/CONACK exceed one 255-byte modem frame, so they arrive fragmented).
    let qsy_frame = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| decode_qsy_frame(text.trim()).ok());
    let Some(frame) = qsy_frame else {
        // Route the reassembly by SAR segment-id (the 4-byte header is public layout): 0 = handshake
        // (unchanged, bit-for-bit), any other id = file transfer. A malformed sub-header frame stays on
        // the handshake path, which ignores it exactly as before.
        match sar_segment_id(bytes) {
            Some(0) | None => {
                try_reassemble_handshake(bytes, runtime_state, event_tx, &mode, engine)
            }
            Some(segment_id) => {
                filexfer::route_inbound_fragment(bytes, segment_id, runtime_state, event_tx, &mode)
            }
        }
        return;
    };

    // Audit F6 (§97.119): the QSY responder keys the transmitter (even a Reject reply is an on-air
    // frame), so an autonomous responder that merely heard a QSY frame must not engage without a
    // valid MYID, or it would transmit unidentified.
    if !runtime_state.local_callsign_valid() {
        tracing::warn!(
            "qsy: ignoring inbound frame — no valid station callsign to transmit an identified reply"
        );
        return;
    }

    // Audit F4: classify the QSY requester's trust from the peer we verified this session (over-air,
    // no PSK → at most `Reduced`) instead of a hardcoded `Unverified`, so `qsy.allow_trustlevels`
    // is an enforceable gate rather than a control that rejects every peer.
    let qsy_policy = runtime_state.qsy_policy.clone();
    let peer_trust = runtime_state.rf_peer_trust();
    let is_new_session = runtime_state.qsy_session.is_none();
    let session = runtime_state
        .qsy_session
        .get_or_insert_with(|| QsySession::new_responder(qsy_policy, peer_trust));

    // Notify connected clients that a remote station initiated QSY.
    if is_new_session {
        if let QsyFrame::Req {
            ref token,
            n_candidates,
        } = frame
        {
            let _ = event_tx.send(ControlEvent::QsyIncoming {
                token: token.clone(),
                n_candidates,
            });
        }
    }

    match session.apply(frame) {
        Ok(actions) => {
            execute_qsy_actions(
                actions,
                session,
                engine,
                rig_controller,
                event_tx,
                &mode,
                runtime_state.qsy_scan_dwell_ms,
            )
            .await;
        }
        Err(e) => tracing::warn!(error = %e, "qsy responder: apply frame failed"),
    }
}

/// Session key for the handshake SAR reassembler. One handshake is in flight per peer connection,
/// and a node only ever receives one frame type at a time (initiator→CONACK, responder→CONREQ).
#[cfg(not(target_arch = "wasm32"))]
const HANDSHAKE_SAR_SESSION: &str = "handshake";

/// SAR-fragment a signed handshake frame (CONREQ/CONACK are ~500 B, over the 255 B modem-frame
/// cap) and transmit each fragment. The receiver reassembles them in [`try_reassemble_handshake`].
#[cfg(not(target_arch = "wasm32"))]
fn transmit_handshake_frame(engine: &mut ModemEngine, mode: &str, frame: &[u8]) {
    let fragments = match sar_encode(0, frame) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "handshake: SAR encode failed");
            return;
        }
    };
    for frag in fragments {
        if let Err(e) = engine.transmit(&frag, mode, None) {
            tracing::warn!(error = %e, "handshake: fragment transmit failed");
        }
    }
}

/// Feed a non-QSY, non-relay frame into the handshake SAR reassembler; on a completed segment,
/// dispatch the reassembled CONREQ/CONACK (confirmed by its HSCQ/HSAK magic). Stray frames create
/// at most a short-lived reassembly slot that the periodic [`expire_pending_handshake`] clears.
#[cfg(not(target_arch = "wasm32"))]
/// The SAR `segment_id` (big-endian bytes 0–1) of a fragment, or `None` if it's too short to be a
/// well-formed SAR fragment. Used to route reassembly (handshake = 0, file transfer ≠ 0).
#[cfg(not(target_arch = "wasm32"))]
fn sar_segment_id(bytes: &[u8]) -> Option<u16> {
    (bytes.len() >= openpulse_core::sar::SAR_HEADER_SIZE)
        .then(|| ((bytes[0] as u16) << 8) | bytes[1] as u16)
}

#[cfg(not(target_arch = "wasm32"))]
fn try_reassemble_handshake(
    bytes: &[u8],
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    engine: &mut ModemEngine,
) {
    let assembled = match runtime_state
        .handshake_sar
        .ingest(HANDSHAKE_SAR_SESSION, bytes)
    {
        Ok(Some(full)) => full,
        Ok(None) => return,
        Err(_) => return, // not a well-formed SAR fragment; ignore
    };
    if assembled.starts_with(b"HSCQ") {
        handle_inbound_conreq(&assembled, runtime_state, event_tx, mode, engine);
    } else if assembled.starts_with(b"HSAK") {
        handle_inbound_conack(&assembled, runtime_state, event_tx);
    } else {
        tracing::debug!("handshake: reassembled segment has no CONREQ/CONACK magic; dropping");
    }
}

/// Responder side of the signed handshake: verify an inbound CONREQ, reply with a signed
/// CONACK over RF, and record the proven peer identity (callsign + grid + pubkey). Verification
/// failures are logged and dropped (no reply), so an unverifiable frame can't open a session.
#[cfg(not(target_arch = "wasm32"))]
fn handle_inbound_conreq(
    bytes: &[u8],
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    mode: &str,
    engine: &mut ModemEngine,
) {
    let req = match ConReq::decode(bytes) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "handshake: CONREQ decode failed");
            return;
        }
    };
    // Permissive policy: the signature proves key possession; trust classification is recorded
    // but an unknown (first-seen) peer is still allowed to connect, mirroring `ConnectPeer`.
    if let Err(e) = verify_conreq(
        &req,
        &runtime_state.trust_store,
        PolicyProfile::Permissive,
        SigningMode::Normal,
    ) {
        tracing::warn!(peer = %req.station_id, error = %e, "handshake: CONREQ verification rejected");
        return;
    }

    // Audit F6 (§97.119): replying with a CONACK keys the transmitter. Auto-ID is disabled without
    // a valid callsign, so an autonomous responder must not answer a CONREQ unidentified — refuse to
    // key up (and don't record a half-handshake the peer never sees completed).
    if !runtime_state.local_callsign_valid() {
        tracing::warn!(
            peer = %req.station_id,
            "handshake: heard a CONREQ but no valid station callsign is set; not transmitting a CONACK"
        );
        return;
    }

    // Reply with a signed CONACK echoing the session id and advertising our grid + OTA ladder.
    let (ota_name, ota_fp) = runtime_state
        .local_ota_ladder
        .clone()
        .unwrap_or_else(|| (String::new(), 0));
    match ConAck::create_full(
        &runtime_state.local_callsign,
        &runtime_state.station_seed,
        SigningMode::Normal,
        &req.session_id,
        openpulse_core::compression::CompressionAlgorithm::None,
        openpulse_core::fec::FecMode::None,
        &runtime_state.local_grid,
        &ota_name,
        ota_fp,
    ) {
        Ok(ack) => match ack.encode() {
            Ok(frame) => transmit_handshake_frame(engine, mode, &frame),
            Err(e) => tracing::warn!(error = %e, "handshake: CONACK encode failed"),
        },
        Err(e) => tracing::warn!(error = %e, "handshake: CONACK create failed"),
    }

    record_verified_peer(
        runtime_state,
        event_tx,
        &req.station_id,
        &req.station_grid,
        &req.pubkey,
        &req.profile_name,
        req.profile_fingerprint,
    );
}

/// Initiator side of the signed handshake: verify the peer's CONACK against the in-flight CONREQ,
/// then record the proven peer identity and clear the pending handshake.
#[cfg(not(target_arch = "wasm32"))]
fn handle_inbound_conack(
    bytes: &[u8],
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    let ack = match ConAck::decode(bytes) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "handshake: CONACK decode failed");
            return;
        }
    };
    let Some(pending) = runtime_state.pending_handshake.clone() else {
        tracing::debug!("handshake: CONACK received with no pending CONREQ; ignoring");
        return;
    };
    // The CONACK must echo our CONREQ's session id. `verify_conack` also re-checks this, but
    // gating here avoids tearing down a pending handshake on an unrelated peer's CONACK.
    if ack.session_id != pending.session_id {
        tracing::debug!(
            got = %ack.session_id,
            want = %pending.session_id,
            "handshake: CONACK session id mismatch; ignoring"
        );
        return;
    }
    // The CONACK must come from the station we actually dialed (audit F2). The session id is cleartext and
    // time-based (guessable within the handshake window), so without this an attacker who races a CONACK
    // echoing it — under their own callsign — would be recorded as the peer the operator meant to reach.
    if ack.station_id != pending.peer_callsign {
        tracing::warn!(
            got = %ack.station_id,
            dialed = %pending.peer_callsign,
            "handshake: CONACK from a different station than dialed; ignoring"
        );
        return;
    }
    if let Err(e) = verify_conack(
        &ack,
        &pending.session_id,
        &[],
        &[],
        &runtime_state.trust_store,
        PolicyProfile::Permissive,
        SigningMode::Normal,
    ) {
        tracing::warn!(peer = %ack.station_id, error = %e, "handshake: CONACK verification rejected");
        runtime_state.pending_handshake = None;
        return;
    }
    record_verified_peer(
        runtime_state,
        event_tx,
        &ack.station_id,
        &ack.station_grid,
        &ack.pubkey,
        &ack.profile_name,
        ack.profile_fingerprint,
    );
    runtime_state.pending_handshake = None;
}

/// Store a freshly-verified peer identity, stamp the verified grid onto the in-flight logbook QSO,
/// and emit a `PeerVerified` event for clients.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn record_verified_peer(
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    callsign: &str,
    grid: &str,
    pubkey: &[u8],
    peer_profile_name: &str,
    peer_profile_fingerprint: u64,
) {
    // Ladder-compatibility guard: compare the peer's advertised OTA ladder identity to ours. Only a
    // definite mismatch (both sides advertised, fingerprints differ) suppresses OTA — an unadvertised
    // side leaves it undetermined (None), so OTA-without-handshake keeps working.
    let profile_compatible = match (&runtime_state.local_ota_ladder, peer_profile_fingerprint) {
        (Some((_, local_fp)), peer_fp) if peer_fp != 0 => Some(*local_fp == peer_fp),
        _ => None,
    };
    if profile_compatible == Some(false) {
        let local_fp = runtime_state
            .local_ota_ladder
            .as_ref()
            .map(|(_, fp)| *fp)
            .unwrap_or(0);
        tracing::warn!(
            peer = %callsign,
            peer_profile = %peer_profile_name,
            peer_fingerprint = format!("{peer_profile_fingerprint:016x}"),
            local_fingerprint = format!("{local_fp:016x}"),
            "handshake: peer OTA rate ladder differs from ours; disabling adaptive OTA (fixed mode)"
        );
    }
    runtime_state.verified_peer = Some(VerifiedPeer {
        callsign: callsign.to_string(),
        grid: grid.to_string(),
        pubkey: pubkey.to_vec(),
        profile_compatible,
    });
    // Prefer the on-air verified grid over the config peer_grids fallback for this QSO.
    if !grid.is_empty() {
        runtime_state.logbook.set_pending_peer_grid(grid);
    }
    tracing::info!(peer = %callsign, grid = %grid, "handshake: peer identity verified");
    let _ = event_tx.send(ControlEvent::PeerVerified {
        callsign: callsign.to_string(),
        grid: grid.to_string(),
    });
}

/// Abandon an unanswered CONREQ once [`HANDSHAKE_TIMEOUT`] elapses. Called from the daemon
/// receive loop each tick; emits a `CommandError` so the operator sees the handshake gave up.
#[cfg(not(target_arch = "wasm32"))]
pub fn expire_pending_handshake(
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    // Drop stale partial reassemblies (e.g. a handshake that lost a fragment).
    runtime_state.handshake_sar.expire();
    if let Some(p) = &runtime_state.pending_handshake {
        if p.started_at.elapsed() >= HANDSHAKE_TIMEOUT {
            let peer = p.peer_callsign.clone();
            runtime_state.pending_handshake = None;
            tracing::warn!(peer = %peer, "handshake: CONACK timed out; no verified identity");
            let _ = event_tx.send(ControlEvent::CommandError {
                command: "connect_peer".to_string(),
                reason: format!("handshake timed out awaiting CONACK from {peer}"),
            });
        }
    }
}

/// Auto-initiate a QSY when the receiver notch confirms a persistent **in-band** interferer — one
/// a notch can't remove. Called from the main loop after each receive tick. No-op unless
/// `auto_enabled`, the engine reports an in-band interferer, candidate frequencies are configured,
/// and no QSY negotiation is already in flight. Reuses the standard initiator path
/// ([`QsySession::initiate`] + [`execute_qsy_actions`]), so the peer responds over RF as usual.
#[cfg(not(target_arch = "wasm32"))]
pub async fn maybe_qsy_on_interference(
    auto_enabled: bool,
    runtime_state: &mut RuntimeControlState,
    rig_controller: Option<&mut (dyn CatController + Send)>,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    active_mode: &SharedMode,
    engine: &mut ModemEngine,
) {
    if !auto_enabled || runtime_state.qsy_session.is_some() {
        return;
    }
    if engine.in_band_interferers().is_empty() {
        return;
    }
    let interferers: Vec<f32> = engine.in_band_interferers().to_vec();
    let candidates = runtime_state.qsy_candidate_freqs.clone();
    if candidates.is_empty() {
        tracing::warn!(
            ?interferers,
            "in-band interference confirmed but no QSY candidates configured (qsy.candidate_freqs_hz); cannot auto-QSY"
        );
        // Clear so the warning doesn't repeat every tick until the tracker decays.
        engine.clear_in_band_interferers();
        return;
    }

    // Audit F6 (§97.119): auto-QSY keys the transmitter to send the QSY request; without a valid
    // MYID the daemon can't auto-ID, so refuse to initiate rather than transmit unidentified.
    if !runtime_state.local_callsign_valid() {
        tracing::warn!(
            ?interferers,
            "in-band interference confirmed but no valid station callsign is set; not auto-initiating QSY"
        );
        engine.clear_in_band_interferers();
        return;
    }

    tracing::warn!(
        ?interferers,
        "in-band interference confirmed — auto-initiating QSY"
    );
    let mut session =
        QsySession::new_initiator().with_switchover_offset_s(runtime_state.qsy_switchover_offset_s);
    let actions = match session.initiate(candidates) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "auto-QSY initiate failed");
            return;
        }
    };
    let mode = active_mode.lock().await.clone();
    execute_qsy_actions(
        actions,
        &mut session,
        engine,
        rig_controller,
        event_tx,
        &mode,
        runtime_state.qsy_scan_dwell_ms,
    )
    .await;
    runtime_state.qsy_session = Some(session);
    // The old interferer no longer applies once we move; start fresh so we don't re-trigger.
    engine.clear_in_band_interferers();
}

fn maybe_relay_forward(
    payload: &[u8],
    mode: &str,
    runtime_state: &mut RuntimeControlState,
    engine: &mut ModemEngine,
) {
    use openpulse_core::wire_query::WireEnvelope;

    // Audit F6 (§97.119): retransmitting keys the transmitter. Without a valid MYID the daemon can't
    // auto-ID, so a relay must not forward (transmit) unidentified.
    if !runtime_state.local_callsign_valid() {
        return;
    }
    let Some(ref mut fwd) = runtime_state.relay_forwarder else {
        return;
    };
    let Ok(envelope) = WireEnvelope::decode(payload) else {
        return;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let forwarded = fwd.forward(&envelope, now_ms);
    match forwarded {
        Ok(out_envelope) => match out_envelope.encode() {
            Ok(out_bytes) => {
                tracing::info!(
                    session_id = ?out_envelope.session_id,
                    hop_index = out_envelope.hop_index,
                    bytes = out_bytes.len(),
                    "relay: forwarding envelope"
                );
                if let Err(e) = engine.transmit(&out_bytes, mode, None) {
                    tracing::warn!(error = %e, "relay: retransmit failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "relay: envelope encode failed"),
        },
        Err(e) => tracing::info!(reason = ?e, "relay: dropping envelope"),
    }
}
///
/// This complements [`dispatch_command`], which updates shared daemon state and
/// forwards commands to the caller. Commands without runtime support emit a
/// [`ControlEvent::CommandError`] instead of failing silently.
#[cfg(not(target_arch = "wasm32"))]
pub async fn apply_command_to_engine(
    cmd: &ControlCommand,
    engine: &mut ModemEngine,
    active_mode: &SharedMode,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    rig_controller: Option<&mut (dyn CatController + Send)>,
    runtime_state: &mut RuntimeControlState,
) {
    match cmd {
        ControlCommand::SetMode { mode } => {
            if engine.plugins().get(mode).is_some() {
                *active_mode.lock().await = mode.clone();
            } else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_mode".to_string(),
                    reason: format!("unsupported mode '{mode}'"),
                });
            }
        }
        ControlCommand::SetTxAttenuation { db, band } => match band {
            // Per-band override: remember it, and apply immediately only when it is the current band.
            Some(label) => {
                runtime_state
                    .tx_attenuation_bands
                    .insert(label.clone(), *db);
                let on_this_band = runtime_state
                    .last_freq_hz
                    .and_then(openpulse_qsy::bandplan::band_label_for_hz)
                    == Some(label.as_str());
                if on_this_band {
                    engine.set_tx_attenuation_db(*db);
                }
            }
            // Global default: takes effect now, unless a per-band override matches the current band.
            None => {
                runtime_state.tx_attenuation_default = *db;
                match runtime_state.last_freq_hz {
                    Some(hz) => apply_band_attenuation(engine, runtime_state, hz),
                    None => engine.set_tx_attenuation_db(*db),
                }
            }
        },
        ControlCommand::SetConfig { config } => {
            if engine.plugins().get(&config.mode).is_some() {
                *active_mode.lock().await = config.mode.clone();
            } else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_config".to_string(),
                    reason: format!("unsupported mode '{}'", config.mode),
                });
            }
            // Sets the global default; per-band overrides persist and re-apply on retune.
            runtime_state.tx_attenuation_default = config.tx_attenuation_db;
            match runtime_state.last_freq_hz {
                Some(hz) => apply_band_attenuation(engine, runtime_state, hz),
                None => engine.set_tx_attenuation_db(config.tx_attenuation_db),
            }
        }
        ControlCommand::PttAssert => {
            // The hardware key happens in the server's `handle_ptt_command`; here we arm the watchdog
            // deadline and announce the logical edge. The independent watchdog thread reads this arm.
            runtime_state.ptt.arm();
            let _ = event_tx.send(ControlEvent::PttChanged { active: true });
        }
        ControlCommand::PttRelease => {
            runtime_state.ptt.disarm();
            let _ = event_tx.send(ControlEvent::PttChanged { active: false });
        }
        ControlCommand::ConnectPeer { callsign } => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            // Use the configured trust level when the peer is in the trust store.
            // Unknown peers (not yet in store) get Full so the session proceeds and
            // trust is established at handshake time.  Revoked peers are rejected.
            let stored_trust = runtime_state.trust_store.trust_level(callsign);
            let key_trust = if stored_trust == PublicKeyTrustLevel::Unknown {
                PublicKeyTrustLevel::Full
            } else {
                stored_trust
            };
            let params = SecureSessionParams {
                local_minimum_mode: SigningMode::Normal,
                peer_supported_modes: vec![SigningMode::Normal, SigningMode::Psk],
                key_trust,
                certificate_source: CertificateSource::OutOfBand,
                psk_validated: false,
            };

            match engine.begin_secure_session(params, now_ms) {
                Ok(_) => {
                    let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                        connected: true,
                        peer: Some(callsign.clone()),
                    });

                    // Open a logbook QSO (finalized + appended on disconnect).
                    let mode = active_mode.lock().await.clone();
                    let freq = runtime_state.last_freq_hz;
                    runtime_state
                        .logbook
                        .begin_qso(callsign, &mode, freq, now_ms);

                    let token = format!("qsy-{now_ms}");
                    runtime_state.qsy_pending_token = Some(token.clone());
                    let _ = event_tx.send(ControlEvent::QsyPending { token });

                    // Initiate the signed handshake over RF: send a CONREQ and await the
                    // peer's CONACK (verified in `process_received_bytes`). Additive to the
                    // local trust eval above — the connection upgrades to "verified" on CONACK.
                    let session_id = format!("{}-{now_ms}", runtime_state.local_callsign);
                    let (ota_name, ota_fp) = runtime_state
                        .local_ota_ladder
                        .clone()
                        .unwrap_or_else(|| (String::new(), 0));
                    match ConReq::create_full(
                        &runtime_state.local_callsign,
                        &runtime_state.station_seed,
                        vec![SigningMode::Normal],
                        &session_id,
                        vec![],
                        vec![],
                        &runtime_state.local_grid,
                        &ota_name,
                        ota_fp,
                    ) {
                        Ok(req) => match req.encode() {
                            Ok(frame) => {
                                transmit_handshake_frame(engine, &mode, &frame);
                                runtime_state.pending_handshake = Some(PendingHandshake {
                                    session_id,
                                    peer_callsign: callsign.clone(),
                                    started_at: Instant::now(),
                                });
                            }
                            Err(e) => tracing::warn!(error = %e, "handshake: CONREQ encode failed"),
                        },
                        Err(e) => tracing::warn!(error = %e, "handshake: CONREQ create failed"),
                    }
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "connect_peer".to_string(),
                        reason: format!("secure session start failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::DisconnectPeer => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            match engine.end_secure_session(now_ms) {
                Ok(()) => {
                    runtime_state.qsy_pending_token = None;
                    // Finalize + append the logbook QSO (opt-in; failures don't affect the session).
                    let rx_snr = engine.last_rx_snr_db();
                    if let Err(e) = runtime_state.logbook.end_qso(now_ms, rx_snr) {
                        tracing::warn!(error = %e, "logbook: failed to append ADIF record");
                    }
                    let _ = event_tx.send(ControlEvent::RfConnectionChanged {
                        connected: false,
                        peer: None,
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "disconnect_peer".to_string(),
                        reason: format!("secure session end failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::SendMessage { body, .. } => {
            // Plain fixed-mode transmit. When an OTA session is active, the daemon's
            // run loop (`server::run`) intercepts SendMessage upstream to drive the
            // receiver-led OTA send with the real-radio PTT turnaround, so this branch
            // only runs for the non-OTA case.
            let mode = active_mode.lock().await.clone();
            // Compress on the wire when enabled; the peer's rx tick unpacks the self-describing frame.
            let payload = if runtime_state.compress_tx {
                openpulse_core::compression::pack(body.as_bytes())
            } else {
                body.as_bytes().to_vec()
            };
            if let Err(err) = engine.transmit(&payload, &mode, None) {
                tracing::warn!(mode = %mode, error = %err, "daemon rf dispatch failed for SendMessage");
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "send_message".to_string(),
                    reason: format!("rf dispatch failed in mode '{mode}': {err}"),
                });
            }
        }
        ControlCommand::SetFreq { rig, freq_hz } => {
            if rig != "rigctld" {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_freq".to_string(),
                    reason: format!("unsupported rig target '{rig}'"),
                });
                return;
            }

            let Some(controller) = rig_controller else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_freq".to_string(),
                    reason: "no rigctld controller configured".to_string(),
                });
                return;
            };

            match controller.set_frequency(*freq_hz) {
                Ok(()) => {
                    runtime_state.last_freq_hz = Some(*freq_hz);
                    // Restore the per-band DCD squelch + TX attenuation for the new frequency.
                    apply_band_squelch(engine, runtime_state, *freq_hz);
                    apply_band_attenuation(engine, runtime_state, *freq_hz);
                    let _ = event_tx.send(ControlEvent::RigStatus {
                        rig: rig.clone(),
                        freq_hz: *freq_hz,
                        mode: "CAT".to_string(),
                        power_w: None,
                        alc: None,
                        swr: None,
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "set_freq".to_string(),
                        reason: format!("rigctld set_frequency failed: {err}"),
                    });
                }
            }
        }
        ControlCommand::AcceptQsy { token } => {
            let token = token.trim();
            if token.is_empty() {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "accept_qsy".to_string(),
                    reason: "empty token".to_string(),
                });
                return;
            }

            if runtime_state.qsy_pending_token.as_deref() != Some(token) {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "accept_qsy".to_string(),
                    reason: format!("unknown pending token '{token}'"),
                });
                return;
            }

            match runtime_state.qsy_decisions.get(token) {
                Some(true) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "accept_qsy".to_string(),
                        reason: format!("token '{token}' already accepted"),
                    });
                }
                Some(false) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "accept_qsy".to_string(),
                        reason: format!("token '{token}' already rejected"),
                    });
                }
                None => {
                    runtime_state.qsy_decisions.insert(token.to_string(), true);
                    runtime_state.qsy_pending_token = None;
                    let _ = event_tx.send(ControlEvent::QsyDecision {
                        token: token.to_string(),
                        accepted: true,
                    });

                    let candidates = runtime_state.qsy_candidate_freqs.clone();
                    if candidates.is_empty() {
                        let _ = event_tx.send(ControlEvent::CommandError {
                            command: "accept_qsy".to_string(),
                            reason: "no candidate frequencies configured (qsy.candidate_freqs_hz)"
                                .to_string(),
                        });
                        return;
                    }

                    let mut session = QsySession::new_initiator()
                        .with_switchover_offset_s(runtime_state.qsy_switchover_offset_s);
                    let actions = match session.initiate(candidates) {
                        Ok(a) => a,
                        Err(e) => {
                            let _ = event_tx.send(ControlEvent::CommandError {
                                command: "accept_qsy".to_string(),
                                reason: format!("QSY session initiate failed: {e}"),
                            });
                            return;
                        }
                    };

                    let mode = active_mode.lock().await.clone();
                    execute_qsy_actions(
                        actions,
                        &mut session,
                        engine,
                        rig_controller,
                        event_tx,
                        &mode,
                        runtime_state.qsy_scan_dwell_ms,
                    )
                    .await;

                    runtime_state.qsy_session = Some(session);
                }
            }
        }
        ControlCommand::RejectQsy { token } => {
            let token = token.trim();
            if token.is_empty() {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "reject_qsy".to_string(),
                    reason: "empty token".to_string(),
                });
                return;
            }

            if runtime_state.qsy_pending_token.as_deref() != Some(token) {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "reject_qsy".to_string(),
                    reason: format!("unknown pending token '{token}'"),
                });
                return;
            }

            match runtime_state.qsy_decisions.get(token) {
                Some(true) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "reject_qsy".to_string(),
                        reason: format!("token '{token}' already accepted"),
                    });
                }
                Some(false) => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "reject_qsy".to_string(),
                        reason: format!("token '{token}' already rejected"),
                    });
                }
                None => {
                    runtime_state.qsy_decisions.insert(token.to_string(), false);
                    runtime_state.qsy_pending_token = None;
                    let _ = event_tx.send(ControlEvent::QsyDecision {
                        token: token.to_string(),
                        accepted: false,
                    });
                }
            }
        }
        ControlCommand::EnableRepeater => {
            if runtime_state.repeater_enabled {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "enable_repeater".to_string(),
                    reason: "repeater already enabled".to_string(),
                });
                return;
            }

            if let Some(mut repeater) = runtime_state.repeater.take() {
                let stop = Arc::new(AtomicBool::new(false));
                let stop_clone = Arc::clone(&stop);
                let thread = std::thread::spawn(move || {
                    if let Err(e) = repeater.run_full_duplex(stop_clone) {
                        tracing::warn!(error = %e, "cross-band repeater exited with error");
                    }
                });
                runtime_state.repeater_stop = Some(stop);
                runtime_state.repeater_thread = Some(thread);
            } else {
                tracing::warn!("enable_repeater: no pre-built repeater in runtime state; audio routing not started");
            }

            runtime_state.repeater_enabled = true;
            let _ = event_tx.send(ControlEvent::RepeaterChanged { enabled: true });
        }
        ControlCommand::DisableRepeater => {
            if !runtime_state.repeater_enabled {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "disable_repeater".to_string(),
                    reason: "repeater already disabled".to_string(),
                });
                return;
            }

            if let Some(stop) = runtime_state.repeater_stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
            if let Some(thread) = runtime_state.repeater_thread.take() {
                let _ = thread.join();
            }

            runtime_state.repeater_enabled = false;
            let _ = event_tx.send(ControlEvent::RepeaterChanged { enabled: false });
        }
        ControlCommand::StartOtaSession { profile } => {
            match openpulse_core::profile::SessionProfile::by_name(profile) {
                Some(p) => {
                    engine.start_ota_session(p);
                    let _ = event_tx.send(ota_status_event(engine));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "start_ota_session".to_string(),
                        reason: format!("unknown profile '{profile}'"),
                    });
                }
            }
        }
        ControlCommand::StopOtaSession => {
            engine.stop_ota_session();
            let _ = event_tx.send(ota_status_event(engine));
        }
        ControlCommand::OtaSetLevelBounds {
            min_level,
            max_level,
        } => {
            let parse = |o: &Option<String>| {
                o.as_deref()
                    .filter(|s| !s.is_empty())
                    .and_then(openpulse_core::rate::SpeedLevel::from_name)
            };
            engine.ota_set_level_bounds(parse(min_level), parse(max_level));
            let _ = event_tx.send(ota_status_event(engine));
        }
        ControlCommand::OtaLockLevel { level } => {
            match openpulse_core::rate::SpeedLevel::from_name(level) {
                Some(l) => {
                    engine.ota_lock_level(l);
                    let _ = event_tx.send(ota_status_event(engine));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "ota_lock_level".to_string(),
                        reason: format!("invalid level '{level}'"),
                    });
                }
            }
        }
        ControlCommand::OtaUnlock => {
            engine.ota_unlock();
            let _ = event_tx.send(ota_status_event(engine));
        }
        ControlCommand::OtaSetHysteresis {
            min_backlog,
            upgrade_hold_frames,
        } => {
            if let Some(b) = min_backlog {
                engine.set_min_backlog_for_upgrade(*b);
            }
            if let Some(f) = upgrade_hold_frames {
                engine.set_upgrade_hold_frames(*f);
            }
        }
        ControlCommand::OtaSetAggressiveness { preset } => {
            match openpulse_core::rate::OtaAggressiveness::from_name(preset) {
                Some(p) => engine.set_ota_aggressiveness(p),
                None => {
                    let _ = event_tx.send(ControlEvent::CommandError {
                        command: "ota_set_aggressiveness".to_string(),
                        reason: format!(
                            "unknown preset '{preset}' (conservative|balanced|aggressive)"
                        ),
                    });
                }
            }
        }
        ControlCommand::SetDcdSquelch { threshold } => {
            if threshold.is_finite() && *threshold >= 0.0 {
                engine.set_dcd_squelch(*threshold);
            } else {
                let _ = event_tx.send(ControlEvent::CommandError {
                    command: "set_dcd_squelch".to_string(),
                    reason: format!("invalid threshold {threshold}"),
                });
            }
        }
        ControlCommand::SetCessb { enabled } => {
            engine.set_cessb_enabled(*enabled);
        }
        ControlCommand::SetNotch { enabled } => {
            if *enabled {
                engine.enable_notch();
            } else {
                engine.disable_notch();
            }
        }
        ControlCommand::SetAgc { enabled } => {
            if *enabled {
                engine.enable_agc();
            } else {
                engine.disable_agc();
            }
        }
        ControlCommand::SetLogbook { enabled } => {
            runtime_state.logbook.set_enabled(*enabled);
        }
        // Receive-side file-transfer decisions act on the active session (engine + event_tx are here).
        ControlCommand::AcceptFile { transfer_id } => {
            let mode = active_mode.lock().await.clone();
            filexfer::accept_offer(*transfer_id, runtime_state, event_tx, &mode);
        }
        ControlCommand::RejectFile { transfer_id } => {
            let mode = active_mode.lock().await.clone();
            filexfer::reject_offer(*transfer_id, runtime_state, event_tx, &mode);
        }
        ControlCommand::CancelFile { transfer_id } => {
            let mode = active_mode.lock().await.clone();
            filexfer::cancel_transfer(*transfer_id, runtime_state, event_tx, &mode);
        }
        ControlCommand::SendFile { to, path } => {
            let mode = active_mode.lock().await.clone();
            filexfer::send_file(to, path, runtime_state, event_tx, &mode);
        }
        ControlCommand::EnableDiscovery => set_discovery_enabled(true, runtime_state, event_tx),
        ControlCommand::DisableDiscovery => set_discovery_enabled(false, runtime_state, event_tx),
        ControlCommand::RendezvousWith { callsign } => {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            start_rendezvous_cmd(callsign, runtime_state, event_tx, now_ms);
        }
        ControlCommand::ListStations => emit_station_list(runtime_state, event_tx),
        ControlCommand::ListPeers => {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            emit_peer_list(runtime_state, event_tx, now_ms);
        }
        ControlCommand::ListFiles => emit_file_list(runtime_state, event_tx),
        ControlCommand::GetPttState => {
            // Re-emit the current PTT state so a client that missed the edge can resync. PTT is keyed
            // exactly when the watchdog deadline is armed (`arm()` on every key path, cleared on
            // release), so it is the single source of truth for the logical PTT state.
            let active = runtime_state.ptt.is_keyed();
            let _ = event_tx.send(ControlEvent::PttChanged { active });
        }
        // No live-modem side effects for these commands in the engine path.
        ControlCommand::SubscribeSpectrum { .. }
        | ControlCommand::GetConfig
        | ControlCommand::ListMessages
        | ControlCommand::GetMessage { .. }
        | ControlCommand::DeleteMessage { .. } => {}
    }
}

/// JS8 discovery lifecycle-state label for [`ControlEvent::DiscoveryStatus`].
pub(crate) fn discovery_state_label(state: openpulse_discovery::DiscoveryState) -> &'static str {
    use openpulse_discovery::DiscoveryState::*;
    match state {
        Inactive => "inactive",
        Activating => "activating",
        Dwelling => "dwelling",
    }
}

/// Emit a [`ControlEvent::DiscoveryStatus`] reflecting the runtime's current state (no-op when
/// discovery is not configured).
pub(crate) fn emit_discovery_status(
    runtime_state: &RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    if let Some(rt) = runtime_state.discovery.as_ref() {
        let _ = event_tx.send(ControlEvent::DiscoveryStatus {
            state: discovery_state_label(rt.state()).to_string(),
            dial_freq_hz: rt.dial_freq_hz(),
            drift_bias_ms: rt.drift_bias_ms(),
        });
    }
}

/// Handle `EnableDiscovery`/`DisableDiscovery`: toggle the runtime and emit a `DiscoveryStatus`, or a
/// `CommandError` when discovery is not configured for this daemon.
fn set_discovery_enabled(
    on: bool,
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    if runtime_state.discovery.is_none() {
        let _ = event_tx.send(ControlEvent::CommandError {
            command: if on {
                "enable_discovery"
            } else {
                "disable_discovery"
            }
            .to_string(),
            reason: "JS8 discovery is not configured ([discovery] enabled = false)".to_string(),
        });
        return;
    }
    if let Some(rt) = runtime_state.discovery.as_mut() {
        let _ = rt.set_enabled(on); // outcome execution (retune) happens in the rx-tick loop
    }
    emit_discovery_status(runtime_state, event_tx);
}

/// Slots to wait for a rendezvous reply *after* our Propose has fully transmitted, before timing out
/// (`N × 15 s` for NORMAL; 16 ≈ 4 min). Must exceed the peer's receive + Accept-over round-trip
/// (~8–10 slots) so a well-behaved exchange never times out.
const RENDEZVOUS_TIMEOUT_SLOTS: u64 = 16;

/// A short (2-char base-36) rendezvous session token derived from the current time.
fn rendezvous_token(now_ms: u64) -> String {
    const B36: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let v = (now_ms % (36 * 36)) as usize;
    let bytes = [B36[v / 36], B36[v % 36]];
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Handle `RendezvousWith`: propose the current band's working channels to `peer` over JS8. Errors when
/// discovery is not configured, has no channels for the band, or is not in a TX-capable mode.
fn start_rendezvous_cmd(
    peer: &str,
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    now_ms: u64,
) {
    let err = |reason: &str| {
        let _ = event_tx.send(ControlEvent::CommandError {
            command: "rendezvous_with".to_string(),
            reason: reason.to_string(),
        });
    };
    let Some(dial) = runtime_state.discovery.as_ref().map(|d| d.dial_freq_hz()) else {
        return err("JS8 discovery is not configured ([discovery] enabled = false)");
    };
    let channels: Vec<u8> = openpulse_qsy::bandplan::band_label_for_hz(dial)
        .and_then(|label| runtime_state.discovery_rendezvous_channels_hz.get(label))
        .map(|v| (0..v.len().min(u8::MAX as usize) as u8).collect())
        .unwrap_or_default();
    if channels.is_empty() {
        return err("no rendezvous channels configured for the current band");
    }
    let token = rendezvous_token(now_ms);
    if let Some(rt) = runtime_state.discovery.as_mut() {
        rt.start_rendezvous(peer, &token, channels, RENDEZVOUS_TIMEOUT_SLOTS);
        if !rt.rendezvous_active() {
            err("rendezvous requires a configured callsign and beacon/full discovery mode");
        }
    }
}

/// Handle `ListStations`: emit a `StationList` from the discovery table (empty when unconfigured).
fn emit_station_list(
    runtime_state: &RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    let stations = runtime_state
        .discovery
        .as_ref()
        .map(|rt| {
            rt.stations()
                .iter()
                .map(|s| crate::protocol::StationSummary {
                    callsign: s.callsign.clone(),
                    grid: s.grid.clone().unwrap_or_default(),
                    snr_db: s.snr_db,
                    heard_count: s.heard_count,
                    last_heard_ms: s.last_heard_ms,
                    is_opulse: s.hint.is_some(),
                })
                .collect()
        })
        .unwrap_or_default();
    let _ = event_tx.send(ControlEvent::StationList { stations });
}

/// Emit this session's received-file list to the requesting client (`ListFiles`).
pub(crate) fn emit_file_list(
    runtime_state: &RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
) {
    let _ = event_tx.send(ControlEvent::FileList {
        files: runtime_state.received_files.clone(),
    });
}

/// Upsert discovery's hinted (OpenPulse-marked) stations into the shared [`PeerCache`] via
/// `station_to_peer_record`. Plain JS8 stations map to `None` and are skipped. Idempotent — safe to
/// call whenever a station is (re)heard.
pub(crate) fn sync_discovered_peers(runtime_state: &mut RuntimeControlState, now_ms: u64) {
    let records: Vec<_> = runtime_state
        .discovery
        .as_ref()
        .map(|rt| {
            rt.stations()
                .iter()
                .filter_map(openpulse_discovery::station_to_peer_record)
                .collect()
        })
        .unwrap_or_default();
    for r in records {
        runtime_state.peer_cache.upsert(r, now_ms);
    }
}

/// Emit the shared cache's recognized OpenPulse peers (sorted by quality) to the requesting client.
pub(crate) fn emit_peer_list(
    runtime_state: &mut RuntimeControlState,
    event_tx: &Arc<broadcast::Sender<ControlEvent>>,
    now_ms: u64,
) {
    use openpulse_core::peer_cache::{TrustFilter, TrustLevel};
    let peers = runtime_state
        .peer_cache
        .query(0, 0, TrustFilter::Any, 256, now_ms)
        .into_iter()
        .map(|r| crate::protocol::PeerSummary {
            peer_id: r.peer_id,
            capability_mask: r.capability_mask,
            route_quality: r.route_quality,
            trust_level: match r.trust_level {
                TrustLevel::Unknown => "unknown",
                TrustLevel::Reduced => "reduced",
                TrustLevel::PskVerified => "psk_verified",
                TrustLevel::Verified => "verified",
            }
            .to_string(),
        })
        .collect();
    let _ = event_tx.send(ControlEvent::PeerList { peers });
}

/// Resolve the DCD squelch for `freq_hz` (per-band override → default) and apply it.
pub fn apply_band_squelch(
    engine: &mut ModemEngine,
    runtime_state: &RuntimeControlState,
    freq_hz: u64,
) {
    let threshold = openpulse_qsy::bandplan::band_label_for_hz(freq_hz)
        .and_then(|label| runtime_state.dcd_squelch_bands.get(label).copied())
        .unwrap_or(runtime_state.dcd_squelch_default);
    engine.set_dcd_squelch(threshold);
}

/// Resolve the TX attenuation for `freq_hz` (per-band override → global default) and apply it.
pub fn apply_band_attenuation(
    engine: &mut ModemEngine,
    runtime_state: &RuntimeControlState,
    freq_hz: u64,
) {
    let atten = openpulse_qsy::bandplan::band_label_for_hz(freq_hz)
        .and_then(|label| runtime_state.tx_attenuation_bands.get(label).copied())
        .unwrap_or(runtime_state.tx_attenuation_default);
    engine.set_tx_attenuation_db(atten);
}

/// Build an [`ControlEvent::OtaStatus`] snapshot from the engine's current OTA state.
pub fn ota_status_event(engine: &ModemEngine) -> ControlEvent {
    ControlEvent::OtaStatus {
        active: engine.ota_active(),
        tx_mode: engine.ota_tx_mode().map(|s| s.to_string()),
        tx_level: engine.ota_tx_level().map(|l| l.name()),
        tx_fec: format!("{:?}", engine.ota_tx_fec()).to_lowercase(),
        rx_recommended_level: engine.ota_rx_recommended_level().map(|l| l.name()),
        rx_confirmed_level: engine.ota_rx_confirmed_level().map(|l| l.name()),
        is_locked: engine.ota_is_locked(),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::field_reassign_with_default)]
mod command_apply_tests {
    use super::*;
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;

    #[test]
    fn compression_ratio_is_none_before_any_payload() {
        assert_eq!(compression_ratio(0, 0), None);
    }

    #[test]
    fn compression_ratio_reports_compressed_over_raw() {
        // 1000 raw → 200 compressed = 0.20 (a 5:1 reduction).
        let r = compression_ratio(1000, 200).unwrap();
        assert!((r - 0.20).abs() < 1e-6, "got {r}");
    }

    #[test]
    fn compression_ratio_tracks_the_real_compressor_on_compressible_data() {
        // Highly compressible payload → the session compressor beats raw, so ratio < 1.
        let payload = vec![0x5Au8; 4096];
        let (compressed, _algo) = openpulse_core::compression::compress_if_smaller(&payload);
        let r = compression_ratio(payload.len() as u64, compressed.len() as u64).unwrap();
        assert!(
            r < 0.5,
            "repeated-byte payload should compress well, got {r}"
        );
    }

    fn test_engine() -> ModemEngine {
        let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
        engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        engine
    }

    #[test]
    fn list_files_reports_this_sessions_received_files() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(8);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();
        rs.received_files.push(crate::protocol::FileSummary {
            name: "report.txt".into(),
            from: "W1AW".into(),
            size: 100,
            verified: true,
            path: "/tmp/report.txt".into(),
            timestamp_secs: 1_700_000_000,
        });

        emit_file_list(&rs, &ev_tx);

        match rx.try_recv().expect("FileList emitted") {
            ControlEvent::FileList { files } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].name, "report.txt");
                assert!(files[0].verified);
            }
            other => panic!("expected FileList, got {other:?}"),
        }
    }

    async fn apply(
        cmd: ControlCommand,
        engine: &mut ModemEngine,
        rs: &mut RuntimeControlState,
        ev: &Arc<broadcast::Sender<ControlEvent>>,
    ) {
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        apply_command_to_engine(&cmd, engine, &active_mode, ev, None, rs).await;
    }

    #[tokio::test]
    async fn front_end_toggle_commands_reach_the_engine() {
        // SetNotch/SetAgc/SetCessb are the cross-cutting RX/TX front-end toggles (audit H1) — assert
        // each dispatch actually flips the engine state, not just serde-parses.
        let mut engine = test_engine();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        apply(
            ControlCommand::SetNotch { enabled: true },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(engine.is_notch_enabled());
        apply(
            ControlCommand::SetNotch { enabled: false },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(!engine.is_notch_enabled());

        apply(
            ControlCommand::SetAgc { enabled: true },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(engine.is_agc_enabled());
        apply(
            ControlCommand::SetAgc { enabled: false },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(!engine.is_agc_enabled());

        apply(
            ControlCommand::SetCessb { enabled: true },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(engine.cessb_enabled());
        apply(
            ControlCommand::SetCessb { enabled: false },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        assert!(!engine.cessb_enabled());
    }

    #[tokio::test]
    async fn set_dcd_squelch_rejects_invalid_threshold() {
        let mut engine = test_engine();
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        apply(
            ControlCommand::SetDcdSquelch { threshold: 0.05 },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        apply(
            ControlCommand::SetDcdSquelch { threshold: -1.0 },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;

        let mut error_for = None;
        while let Ok(e) = rx.try_recv() {
            if let ControlEvent::CommandError { command, .. } = e {
                error_for = Some(command);
            }
        }
        assert_eq!(error_for.as_deref(), Some("set_dcd_squelch"));
    }

    #[tokio::test]
    async fn ptt_commands_track_state_and_emit_changed() {
        let mut engine = test_engine();
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        apply(ControlCommand::PttAssert, &mut engine, &mut rs, &ev).await;
        assert!(rs.ptt.is_keyed());
        apply(ControlCommand::PttRelease, &mut engine, &mut rs, &ev).await;
        assert!(!rs.ptt.is_keyed());

        let mut states = Vec::new();
        while let Ok(e) = rx.try_recv() {
            if let ControlEvent::PttChanged { active } = e {
                states.push(active);
            }
        }
        assert_eq!(states, vec![true, false]);
    }

    #[tokio::test]
    async fn ota_set_level_bounds_emits_status() {
        let mut engine = test_engine();
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        apply(
            ControlCommand::StartOtaSession {
                profile: "hpx500".into(),
            },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;
        while rx.try_recv().is_ok() {} // drain the start status

        apply(
            ControlCommand::OtaSetLevelBounds {
                min_level: Some("SL3".into()),
                max_level: Some("SL8".into()),
            },
            &mut engine,
            &mut rs,
            &ev,
        )
        .await;

        let mut got_status = false;
        while let Ok(e) = rx.try_recv() {
            if matches!(e, ControlEvent::OtaStatus { .. }) {
                got_status = true;
            }
        }
        assert!(got_status, "OtaSetLevelBounds must emit an OtaStatus");
    }

    #[tokio::test]
    async fn ota_commands_start_lock_and_report_status() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        // Start an OTA session.
        apply_command_to_engine(
            &ControlCommand::StartOtaSession {
                profile: "hpx500".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(engine.ota_active());
        assert_eq!(engine.ota_tx_level().map(|l| l.name()), Some("SL2".into()));

        // Lock to SL4 → status reflects the lock.
        apply_command_to_engine(
            &ControlCommand::OtaLockLevel {
                level: "SL4".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(engine.ota_is_locked());
        assert_eq!(engine.ota_tx_level().map(|l| l.name()), Some("SL4".into()));

        // An OtaStatus event was emitted with the locked state.
        let mut saw_locked_status = false;
        while let Ok(ev) = rx.try_recv() {
            if let ControlEvent::OtaStatus {
                is_locked,
                tx_level,
                ..
            } = ev
            {
                if is_locked && tx_level.as_deref() == Some("SL4") {
                    saw_locked_status = true;
                }
            }
        }
        assert!(
            saw_locked_status,
            "expected an OtaStatus event with the SL4 lock"
        );

        // Unlock + stop.
        apply_command_to_engine(
            &ControlCommand::OtaUnlock,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(!engine.ota_is_locked());
        apply_command_to_engine(
            &ControlCommand::StopOtaSession,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(!engine.ota_active());
    }

    #[test]
    fn ladder_fingerprint_mismatch_suppresses_ota() {
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState {
            local_ota_ladder: Some(("hpx_hf".into(), 0xAAAA_AAAA_AAAA_AAAA)),
            ..RuntimeControlState::default()
        };
        let key = [7u8; 32];

        // Matching fingerprint → compatible, OTA not suppressed.
        record_verified_peer(
            &mut rs,
            &ev_tx,
            "W1AW",
            "",
            &key,
            "hpx_hf",
            0xAAAA_AAAA_AAAA_AAAA,
        );
        assert_eq!(
            rs.verified_peer.as_ref().unwrap().profile_compatible,
            Some(true)
        );
        assert!(!rs.ota_suppressed_by_peer());

        // Diverged ladder (different fingerprint) → incompatible, OTA suppressed.
        record_verified_peer(
            &mut rs,
            &ev_tx,
            "W1AW",
            "",
            &key,
            "hpx_hf",
            0xBBBB_BBBB_BBBB_BBBB,
        );
        assert_eq!(
            rs.verified_peer.as_ref().unwrap().profile_compatible,
            Some(false)
        );
        assert!(
            rs.ota_suppressed_by_peer(),
            "diverged ladder must suppress OTA"
        );

        // Peer advertised no ladder (fp=0) → undetermined, NOT suppressed (OTA-without-handshake case).
        record_verified_peer(&mut rs, &ev_tx, "W1AW", "", &key, "", 0);
        assert_eq!(rs.verified_peer.as_ref().unwrap().profile_compatible, None);
        assert!(!rs.ota_suppressed_by_peer());

        // We have no local OTA ladder → undetermined even if the peer advertises one.
        rs.local_ota_ladder = None;
        record_verified_peer(
            &mut rs,
            &ev_tx,
            "W1AW",
            "",
            &key,
            "hpx_hf",
            0xAAAA_AAAA_AAAA_AAAA,
        );
        assert_eq!(rs.verified_peer.as_ref().unwrap().profile_compatible, None);
        assert!(!rs.ota_suppressed_by_peer());
    }

    #[tokio::test]
    async fn ota_set_hysteresis_dispatches_without_disturbing_session() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        apply_command_to_engine(
            &ControlCommand::StartOtaSession {
                profile: "hpx500".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        let level_before = engine.ota_tx_level();

        // Tuning the anti-oscillation gates must not touch the level or the session.
        apply_command_to_engine(
            &ControlCommand::OtaSetHysteresis {
                min_backlog: Some(256),
                upgrade_hold_frames: Some(4),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(engine.ota_active());
        assert_eq!(engine.ota_tx_level(), level_before);
    }

    #[test]
    fn apply_band_squelch_uses_per_band_override_else_default() {
        let mut engine = test_engine();
        let mut rs = RuntimeControlState {
            dcd_squelch_default: 0.01,
            ..RuntimeControlState::default()
        };
        rs.dcd_squelch_bands.insert("40m".into(), 0.05);

        // 40m is in the map → its override applies.
        apply_band_squelch(&mut engine, &rs, 7_040_000);
        assert!((engine.dcd_squelch() - 0.05).abs() < 1e-6);

        // 20m is not in the map → fall back to the default.
        apply_band_squelch(&mut engine, &rs, 14_070_000);
        assert!((engine.dcd_squelch() - 0.01).abs() < 1e-6);

        // Out-of-band frequency → default.
        apply_band_squelch(&mut engine, &rs, 5_000_000);
        assert!((engine.dcd_squelch() - 0.01).abs() < 1e-6);
    }

    #[test]
    fn apply_band_attenuation_uses_per_band_override_else_default() {
        let mut engine = test_engine();
        let mut rs = RuntimeControlState {
            tx_attenuation_default: -3.0,
            ..RuntimeControlState::default()
        };
        rs.tx_attenuation_bands.insert("40m".into(), -6.0);

        // 40m is in the map → its override applies.
        apply_band_attenuation(&mut engine, &rs, 7_040_000);
        assert!((engine.tx_attenuation_db() - (-6.0)).abs() < 1e-6);

        // 20m is not in the map → fall back to the global default.
        apply_band_attenuation(&mut engine, &rs, 14_070_000);
        assert!((engine.tx_attenuation_db() - (-3.0)).abs() < 1e-6);

        // Out-of-band frequency → default.
        apply_band_attenuation(&mut engine, &rs, 5_000_000);
        assert!((engine.tx_attenuation_db() - (-3.0)).abs() < 1e-6);
    }

    #[tokio::test]
    async fn set_tx_attenuation_per_band_stores_and_applies_on_the_matching_band() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        // Per-band override while not on that band: stored, not applied (engine stays at 0).
        apply_command_to_engine(
            &ControlCommand::SetTxAttenuation {
                db: -6.0,
                band: Some("20m".into()),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert_eq!(rs.tx_attenuation_bands.get("20m").copied(), Some(-6.0));
        assert!(
            (engine.tx_attenuation_db() - 0.0).abs() < 1e-6,
            "override not applied off-band"
        );

        // Now on 20m: a retune applies the stored override.
        rs.last_freq_hz = Some(14_070_000);
        apply_band_attenuation(&mut engine, &rs, 14_070_000);
        assert!((engine.tx_attenuation_db() - (-6.0)).abs() < 1e-6);

        // A global default set while on 20m: the per-band override still wins.
        apply_command_to_engine(
            &ControlCommand::SetTxAttenuation {
                db: -3.0,
                band: None,
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!((rs.tx_attenuation_default - (-3.0)).abs() < 1e-6);
        assert!(
            (engine.tx_attenuation_db() - (-6.0)).abs() < 1e-6,
            "the 20m override wins over the global default while on 20m"
        );

        // Move to a band with no override → the global default applies.
        rs.last_freq_hz = Some(7_040_000);
        apply_band_attenuation(&mut engine, &rs, 7_040_000);
        assert!((engine.tx_attenuation_db() - (-3.0)).abs() < 1e-6);
    }

    #[tokio::test]
    async fn ota_set_aggressiveness_valid_and_invalid() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        // A valid preset dispatches cleanly (no CommandError).
        apply_command_to_engine(
            &ControlCommand::OtaSetAggressiveness {
                preset: "aggressive".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(
            rx.try_recv().is_err(),
            "valid preset must not emit an event"
        );

        // An unknown preset emits a CommandError.
        apply_command_to_engine(
            &ControlCommand::OtaSetAggressiveness {
                preset: "turbo".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        match rx.try_recv() {
            Ok(ControlEvent::CommandError { command, reason }) => {
                assert_eq!(command, "ota_set_aggressiveness");
                assert!(reason.contains("turbo"));
            }
            other => panic!("expected CommandError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_set_config_updates_mode_and_tx_attenuation() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SetConfig {
            config: protocol::DaemonConfig {
                callsign: "N0CALL".into(),
                grid_square: "AA00".into(),
                mode: "BPSK250".into(),
                tx_attenuation_db: -6.0,
                qsy_enabled: false,
                bandplan_mode: "unrestricted".into(),
                allow_tuner_on_high_swr: false,
            },
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert_eq!(*active_mode.lock().await, "BPSK250");
        assert!((engine.tx_attenuation_db() - (-6.0)).abs() < 1e-6);
    }

    #[tokio::test]
    async fn received_bytes_route_opfx_to_filexfer_and_handshake_stays_untouched() {
        use openpulse_core::sar::sar_encode;
        use openpulse_filexfer::{FxFrame, Reason};

        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut rs = RuntimeControlState::default();

        // A handshake fragment (segment-id 0) must stay on the handshake path — filexfer untouched.
        let hs = sar_encode(0, b"HSCQ not a real conreq").unwrap();
        process_received_bytes(&hs[0], &mut rs, None, &ev_tx, &active_mode, &mut engine).await;
        assert_eq!(
            rs.filexfer_frames_routed, 0,
            "handshake must not reach the file seam"
        );

        // An OPFX control fragment (segment-id 0xFFFF) must route to the file-transfer seam.
        let frame = FxFrame::FileReject {
            transfer_id: 1,
            reason: Reason::Busy,
        }
        .encode();
        let ctrl = sar_encode(filexfer::FX_CONTROL_SEGMENT_ID, &frame).unwrap();
        process_received_bytes(&ctrl[0], &mut rs, None, &ev_tx, &active_mode, &mut engine).await;
        assert_eq!(
            rs.filexfer_frames_routed, 1,
            "OPFX control frame must reach the file seam"
        );

        // A block-data fragment (segment-id block_index+1) also routes to the file seam.
        let block = sar_encode(3, b"OPFX block-ish").unwrap();
        process_received_bytes(&block[0], &mut rs, None, &ev_tx, &active_mode, &mut engine).await;
        assert_eq!(
            rs.filexfer_frames_routed, 2,
            "OPFX block fragment must reach the file seam"
        );
    }

    #[tokio::test]
    async fn inbound_offer_and_blocks_write_verified_file() {
        use crate::filexfer::{FileTransferPolicy, FX_CONTROL_SEGMENT_ID};
        use ed25519_dalek::SigningKey;
        use openpulse_core::manifest::TransferManifest;
        use openpulse_core::sar::sar_encode;
        use openpulse_filexfer::{encode_block, split_blocks, FileOffer, FxFrame};

        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(128);
        let ev_tx = Arc::new(tx);

        // A signed offer from a known sender.
        let mut seed = [0u8; 32];
        seed[0] = 42;
        let pubkey = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        let file = b"file transfer receive test payload. ".repeat(80).to_vec();
        let manifest = TransferManifest::sign(&file, "W1AW", &seed).unwrap();
        let transfer_id = 0x0BAD_F00D;
        let block_size = 1024u32;
        let offer =
            FileOffer::from_manifest(transfer_id, &manifest, "recv.txt", "text/plain", block_size)
                .unwrap();
        assert!(offer.block_count >= 2, "want a multi-block file");

        let dir = std::env::temp_dir().join(format!("opfx_recv_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let policy = FileTransferPolicy::from_config(&openpulse_config::FileTransferConfig {
            enabled: true,
            download_dir: dir.to_string_lossy().into_owned(),
            auto_accept_max_bytes: u64::MAX,
            max_file_bytes: 10 * 1024 * 1024,
            per_peer_quota_bytes: 0,
            require_verified_peer: true,
            allowed_peers: vec![],
            offer_timeout_secs: 120,
            partial_ttl_hours: 72,
            burst_max_secs: 20.0,
        });
        let mut rs = RuntimeControlState {
            verified_peer: Some(VerifiedPeer {
                callsign: "W1AW".into(),
                grid: String::new(),
                pubkey: pubkey.to_vec(),
                profile_compatible: None,
            }),
            filexfer_policy: policy,
            ..RuntimeControlState::default()
        };

        // Offer → auto-accepted → receive session started.
        let offer_frag =
            sar_encode(FX_CONTROL_SEGMENT_ID, &FxFrame::FileOffer(offer).encode()).unwrap();
        process_received_bytes(
            &offer_frag[0],
            &mut rs,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;
        assert!(
            rs.file_rx.is_some(),
            "offer should auto-accept and start a session"
        );

        // Deliver every block's fragments.
        for (k, block) in split_blocks(&file, block_size).iter().enumerate() {
            for frag in encode_block(transfer_id, k as u16, block, None).unwrap() {
                process_received_bytes(&frag, &mut rs, None, &ev_tx, &active_mode, &mut engine)
                    .await;
            }
        }

        // The file must have been verified and written; find the FileReceived event + read it back.
        let mut path = None;
        while let Ok(ev) = rx.try_recv() {
            if let ControlEvent::FileReceived {
                verified, path: p, ..
            } = ev
            {
                assert!(verified, "a signed file must verify");
                path = Some(p);
            }
        }
        let path = path.expect("FileReceived emitted");
        assert_eq!(std::fs::read(&path).expect("file on disk"), file);
        assert!(rs.file_rx.is_none(), "session cleared after completion");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn send_file_offers_then_completes_on_receiver_frames() {
        use crate::filexfer::{FileTransferPolicy, FX_CONTROL_SEGMENT_ID};
        use openpulse_core::sar::sar_encode;
        use openpulse_filexfer::{CompleteStatus, FxFrame};

        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(128);
        let ev_tx = Arc::new(tx);

        let dir = std::env::temp_dir().join(format!("opfx_send_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("outbound.txt");
        let contents = b"send side file transfer test ".repeat(4);
        std::fs::write(&file_path, &contents).unwrap();

        let policy = FileTransferPolicy::from_config(&openpulse_config::FileTransferConfig {
            enabled: true,
            download_dir: dir.to_string_lossy().into_owned(),
            auto_accept_max_bytes: 0,
            max_file_bytes: 1 << 20,
            per_peer_quota_bytes: 0,
            require_verified_peer: false,
            allowed_peers: vec![],
            offer_timeout_secs: 120,
            partial_ttl_hours: 72,
            burst_max_secs: 20.0,
        });
        let mut rs = RuntimeControlState {
            local_callsign: "N0CALL".into(),
            filexfer_policy: policy,
            ..RuntimeControlState::default()
        };

        // SendFile → transmit the offer + open the send session.
        let cmd = ControlCommand::SendFile {
            to: "W1AW".into(),
            path: file_path.to_string_lossy().into_owned(),
        };
        apply_command_to_engine(&cmd, &mut engine, &active_mode, &ev_tx, None, &mut rs).await;
        let fx = rs.file_tx.as_ref().expect("send session started");
        let transfer_id = fx.transfer_id();
        assert_eq!(fx.block_count(), 1);

        let feed = |frame: Vec<u8>| sar_encode(FX_CONTROL_SEGMENT_ID, &frame).unwrap()[0].clone();
        // Receiver accepts → send block 0.
        process_received_bytes(
            &feed(
                FxFrame::FileAccept {
                    transfer_id,
                    have_bitmap: vec![],
                }
                .encode(),
            ),
            &mut rs,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;
        // Receiver acks block 0 → awaiting verify.
        process_received_bytes(
            &feed(
                FxFrame::BlockAck {
                    transfer_id,
                    block_index: 0,
                    complete: true,
                    missing_frag_bitmap: vec![],
                }
                .encode(),
            ),
            &mut rs,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;
        // Receiver confirms verified → FileSent.
        process_received_bytes(
            &feed(
                FxFrame::FileComplete {
                    transfer_id,
                    status: CompleteStatus::VerifiedOk,
                    countersignature: [0u8; 64],
                }
                .encode(),
            ),
            &mut rs,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        let mut sent = false;
        while let Ok(ev) = rx.try_recv() {
            if let ControlEvent::FileSent {
                receipt_valid, to, ..
            } = ev
            {
                assert_eq!(receipt_valid, Some(true));
                assert_eq!(to, "W1AW");
                sent = true;
            }
        }
        assert!(sent, "FileSent emitted");
        assert!(
            rs.file_tx.is_none(),
            "send session cleared after completion"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn apply_send_message_transmits_payload_over_active_mode() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "status".into(),
            body: "rf body payload".into(),
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        let rx = engine.receive("BPSK250", None).unwrap();
        assert_eq!(rx, b"rf body payload");
    }

    #[tokio::test]
    async fn apply_send_message_compresses_the_wire_when_enabled() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let body = "status ok ".repeat(20); // compressible
        let cmd = ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "status".into(),
            body: body.clone(),
        };

        let mut runtime_state = RuntimeControlState::default();
        runtime_state.compress_tx = true;
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        // The wire carries a packed (smaller) frame; unpack recovers the original body.
        let rx = engine.receive("BPSK250", None).unwrap();
        assert!(
            rx.len() < body.len(),
            "packed wire frame {} should be smaller than body {}",
            rx.len(),
            body.len()
        );
        assert_eq!(
            openpulse_core::compression::unpack(&rx).unwrap(),
            body.as_bytes()
        );
    }

    #[tokio::test]
    async fn apply_send_message_invalid_mode_emits_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("NO_SUCH_MODE".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cmd = ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "status".into(),
            body: "rf body payload".into(),
        };

        let mut runtime_state = RuntimeControlState::default();
        apply_command_to_engine(
            &cmd,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        let mut saw_error = false;
        while let Ok(ev) = rx.try_recv() {
            if let ControlEvent::CommandError { command, reason } = ev {
                assert_eq!(command, "send_message");
                assert!(reason.contains("NO_SUCH_MODE"));
                saw_error = true;
                break;
            }
        }
        assert!(saw_error, "expected command_error event");
    }

    #[tokio::test]
    async fn apply_unimplemented_runtime_commands_emit_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        let cases = vec![
            (
                ControlCommand::SetFreq {
                    rig: "rigctld".into(),
                    freq_hz: 7_100_000,
                },
                "set_freq",
                "no rigctld controller configured",
            ),
            (
                ControlCommand::AcceptQsy {
                    token: "tok-1".into(),
                },
                "accept_qsy",
                "unknown pending token",
            ),
            (
                ControlCommand::RejectQsy {
                    token: "tok-1".into(),
                },
                "reject_qsy",
                "unknown pending token",
            ),
        ];

        let mut runtime_state = RuntimeControlState::default();
        for (cmd, expected_command, expected_reason_substr) in cases {
            apply_command_to_engine(
                &cmd,
                &mut engine,
                &active_mode,
                &ev_tx,
                None,
                &mut runtime_state,
            )
            .await;
            let ev = rx.recv().await.expect("expected command_error event");
            match ev {
                ControlEvent::CommandError { command, reason } => {
                    assert_eq!(command, expected_command);
                    assert!(reason.contains(expected_reason_substr));
                }
                other => panic!("expected command_error event, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn apply_repeater_enable_disable_emits_state_changes() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        apply_command_to_engine(
            &ControlCommand::EnableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(runtime_state.repeater_enabled);
        match rx.recv().await.expect("expected repeater event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }

        apply_command_to_engine(
            &ControlCommand::DisableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(!runtime_state.repeater_enabled);
        match rx.recv().await.expect("expected repeater event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(!enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_qsy_accept_reject_record_and_emit_decisions() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-accept".to_string());
        runtime_state.qsy_candidate_freqs = vec![14_070_000, 14_077_000];

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-accept".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert_eq!(runtime_state.qsy_decisions.get("tok-accept"), Some(&true));
        assert!(runtime_state.qsy_pending_token.is_none());
        match rx.recv().await.expect("expected qsy event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-accept");
                assert!(accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }

        runtime_state.qsy_pending_token = Some("tok-reject".to_string());
        apply_command_to_engine(
            &ControlCommand::RejectQsy {
                token: "tok-reject".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert_eq!(runtime_state.qsy_decisions.get("tok-reject"), Some(&false));
        assert!(runtime_state.qsy_pending_token.is_none());
        match rx.recv().await.expect("expected qsy event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-reject");
                assert!(!accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn connect_then_disconnect_writes_an_adif_logbook_record() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("QPSK500".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);

        let path = std::env::temp_dir().join(format!("opadif-it-{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut runtime_state = RuntimeControlState {
            logbook: crate::logbook::Logbook::new(
                true,
                path.to_str().unwrap(),
                "DL0XYZ",
                "AA00aa",
                &std::collections::BTreeMap::from([("dl1abc".to_string(), "JO31aa".to_string())]),
            ),
            last_freq_hz: Some(14_070_000),
            ..RuntimeControlState::default()
        };

        for cmd in [
            ControlCommand::ConnectPeer {
                callsign: "DL1ABC".to_string(),
            },
            ControlCommand::DisconnectPeer,
        ] {
            apply_command_to_engine(
                &cmd,
                &mut engine,
                &active_mode,
                &ev_tx,
                None,
                &mut runtime_state,
            )
            .await;
        }

        let body = std::fs::read_to_string(&path).expect("logbook file written");
        assert!(body.contains("<CALL:6>DL1ABC"));
        assert!(body.contains("<BAND:3>20m"));
        assert!(body.contains("<SUBMODE:7>QPSK500"));
        assert!(body.contains("<STATION_CALLSIGN:6>DL0XYZ"));
        assert!(body.contains("<GRIDSQUARE:6>JO31aa")); // worked station's grid from peer_grids
        assert_eq!(body.matches("<EOR>").count(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn apply_connect_disconnect_drive_secure_session_and_pending_qsy() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        apply_command_to_engine(
            &ControlCommand::ConnectPeer {
                callsign: "W1AW".to_string(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(engine.hpx_session_id().is_some());
        let token = runtime_state
            .qsy_pending_token
            .clone()
            .expect("connect should create pending qsy token");

        let first = rx.recv().await.expect("expected event");
        let second = rx.recv().await.expect("expected event");
        let saw_connected = matches!(
            (&first, &second),
            (
                ControlEvent::RfConnectionChanged {
                    connected: true,
                    peer: Some(_)
                },
                ControlEvent::QsyPending { .. }
            ) | (
                ControlEvent::QsyPending { .. },
                ControlEvent::RfConnectionChanged {
                    connected: true,
                    peer: Some(_)
                }
            )
        );
        assert!(
            saw_connected,
            "expected rf connected and qsy pending events"
        );

        apply_command_to_engine(
            &ControlCommand::AcceptQsy { token },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        apply_command_to_engine(
            &ControlCommand::DisconnectPeer,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(runtime_state.qsy_pending_token.is_none());
        assert!(engine.hpx_session_id().is_none());
    }

    #[tokio::test]
    async fn get_ptt_state_rebroadcasts_the_current_keyed_state() {
        // A client that missed a PttChanged edge can resync: GetPttState re-emits the current state,
        // which is keyed exactly when the watchdog deadline is armed.
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        // Currently keyed → GetPttState reports active: true.
        runtime_state.ptt.arm();
        apply_command_to_engine(
            &ControlCommand::GetPttState,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(
            matches!(rx.try_recv(), Ok(ControlEvent::PttChanged { active: true })),
            "GetPttState must re-broadcast active: true while keyed"
        );

        // Not keyed → active: false.
        runtime_state.ptt.disarm();
        apply_command_to_engine(
            &ControlCommand::GetPttState,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;
        assert!(
            matches!(
                rx.try_recv(),
                Ok(ControlEvent::PttChanged { active: false })
            ),
            "GetPttState must re-broadcast active: false while unkeyed"
        );
    }

    #[tokio::test]
    async fn accept_qsy_with_candidates_initiates_session_and_transmits_req() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-qsy".to_string());
        runtime_state.qsy_candidate_freqs = vec![14_070_000, 14_077_000];

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-qsy".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert_eq!(runtime_state.qsy_decisions.get("tok-qsy"), Some(&true));
        assert!(runtime_state.qsy_pending_token.is_none());
        assert!(
            runtime_state.qsy_session.is_some(),
            "QsySession should be stored in runtime_state"
        );

        // QsyDecision event must be first
        match rx.recv().await.expect("expected QsyDecision event") {
            ControlEvent::QsyDecision { token, accepted } => {
                assert_eq!(token, "tok-qsy");
                assert!(accepted);
            }
            other => panic!("expected QsyDecision, got {other:?}"),
        }

        // QSY_REQ and QSY_LIST frames were transmitted; verify loopback receive contains QSY text
        let bytes = engine.receive("BPSK250", None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("QSY_REQ") || text.contains("QSY_LIST"),
            "expected QSY frame in modem output, got: {text:?}"
        );
    }

    #[tokio::test]
    async fn accept_qsy_without_candidates_emits_command_error() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.qsy_pending_token = Some("tok-nocand".to_string());
        // qsy_candidate_freqs is empty (default)

        apply_command_to_engine(
            &ControlCommand::AcceptQsy {
                token: "tok-nocand".into(),
            },
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        // QsyDecision event first, then CommandError for no candidates
        let ev1 = rx.recv().await.expect("expected first event");
        assert!(
            matches!(ev1, ControlEvent::QsyDecision { .. }),
            "expected QsyDecision"
        );
        let ev2 = rx.recv().await.expect("expected CommandError event");
        match ev2 {
            ControlEvent::CommandError { command, reason } => {
                assert_eq!(command, "accept_qsy");
                assert!(reason.contains("candidate"), "reason: {reason}");
            }
            other => panic!("expected CommandError, got {other:?}"),
        }
    }

    /// Drive a persistent in-band CW tone through the streaming capture path until the notch
    /// persistence tracker confirms it, leaving `in_band_interferers()` populated.
    #[cfg(not(target_arch = "wasm32"))]
    fn engine_with_confirmed_in_band_interferer(min_hits: u32) -> ModemEngine {
        let mut engine = test_engine();
        engine.enable_notch();
        engine.set_notch_persistence(min_hits);
        // 1500 Hz = engine centre; BPSK250 occupied 500 Hz → protected band ~1250–1750.
        let tone: Vec<f32> = (0..8192)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        for _ in 0..(min_hits + 1) {
            let _ = engine.accumulate_capture(Some("BPSK250"), tone.clone());
        }
        assert!(
            !engine.in_band_interferers().is_empty(),
            "persistence should confirm the in-band tone"
        );
        // The notch must have actually run on the daemon's streaming (accumulate_capture) path.
        assert!(engine.notch_blocks_processed() > 0);
        engine
    }

    #[tokio::test]
    async fn auto_qsy_on_interference_initiates_session_and_transmits_req() {
        let mut engine = engine_with_confirmed_in_band_interferer(3);
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();
        runtime_state.local_callsign = "W1AW".into(); // valid MYID so auto-QSY may key up (audit F6)
        runtime_state.qsy_candidate_freqs = vec![14_070_000, 14_077_000];

        maybe_qsy_on_interference(
            true,
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_some(),
            "a confirmed in-band interferer should auto-initiate a QSY session"
        );
        assert!(
            engine.in_band_interferers().is_empty(),
            "the hint should be cleared so it does not re-trigger every tick"
        );
        let bytes = engine.receive("BPSK250", None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("QSY_REQ") || text.contains("QSY_LIST"),
            "expected a QSY frame in the modem output, got: {text:?}"
        );
    }

    #[tokio::test]
    async fn auto_qsy_end_to_end_initiator_to_responder_over_rf() {
        use bpsk_plugin::BpskPlugin;
        use openpulse_modem::channel_sim::ChannelSimHarness;

        // Station A (tx_engine) detects the interferer and auto-initiates QSY; Station B (rx_engine)
        // receives the QSY_REQ over the (clean) channel and opens a responder session — the full
        // notch → in-band-interferer → auto-QSY → RF handoff loop, deterministically.
        let mut h = ChannelSimHarness::new();
        h.tx_engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .unwrap();
        h.rx_engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .unwrap();

        // A: confirm a persistent in-band tone via the streaming capture path.
        h.tx_engine.enable_notch();
        h.tx_engine.set_notch_persistence(3);
        let tone: Vec<f32> = (0..8192)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        for _ in 0..4 {
            let _ = h
                .tx_engine
                .accumulate_capture(Some("BPSK250"), tone.clone());
        }
        assert!(!h.tx_engine.in_band_interferers().is_empty());

        // A: auto-QSY → transmits QSY_REQ into the TX loopback.
        let mode_a: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx_a, _rx_a) = broadcast::channel::<ControlEvent>(16);
        let ev_a = Arc::new(tx_a);
        let mut rs_a = RuntimeControlState {
            local_callsign: "W1AW".into(), // valid MYID so auto-QSY may key up (audit F6)
            qsy_candidate_freqs: vec![14_070_000, 14_077_000],
            ..RuntimeControlState::default()
        };
        maybe_qsy_on_interference(true, &mut rs_a, None, &ev_a, &mode_a, &mut h.tx_engine).await;
        assert!(rs_a.qsy_session.is_some(), "A should have initiated QSY");

        // Carry A's transmitted QSY_REQ across the (clean) channel to B and decode it.
        h.route_clean();
        let bytes = h.rx_engine.receive("BPSK250", None).unwrap_or_default();
        assert!(
            String::from_utf8_lossy(&bytes).contains("QSY_REQ"),
            "B should receive A's QSY_REQ, got {:?}",
            String::from_utf8_lossy(&bytes)
        );

        // B: the decoded QSY_REQ drives a responder session.
        let mode_b: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx_b, mut rx_b) = broadcast::channel::<ControlEvent>(16);
        let ev_b = Arc::new(tx_b);
        let mut rs_b = RuntimeControlState {
            local_callsign: "K2XYZ".into(), // valid MYID so the responder may key up (audit F6)
            ..RuntimeControlState::default()
        };
        process_received_bytes(&bytes, &mut rs_b, None, &ev_b, &mode_b, &mut h.rx_engine).await;
        assert!(
            rs_b.qsy_session.is_some(),
            "B should open a responder session from A's auto-QSY QSY_REQ"
        );
        assert!(
            matches!(rx_b.try_recv(), Ok(ControlEvent::QsyIncoming { .. })),
            "B should emit QsyIncoming"
        );
    }

    #[tokio::test]
    async fn auto_qsy_noop_when_disabled_or_session_in_flight() {
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);

        // Disabled → no session even with a confirmed interferer and candidates.
        let mut engine = engine_with_confirmed_in_band_interferer(2);
        let mut rs = RuntimeControlState::default();
        rs.qsy_candidate_freqs = vec![14_070_000];
        maybe_qsy_on_interference(false, &mut rs, None, &ev_tx, &active_mode, &mut engine).await;
        assert!(rs.qsy_session.is_none(), "disabled must not initiate");

        // A negotiation already in flight → don't start another.
        let mut engine2 = engine_with_confirmed_in_band_interferer(2);
        let mut rs2 = RuntimeControlState::default();
        rs2.qsy_candidate_freqs = vec![14_070_000];
        rs2.qsy_session = Some(QsySession::new_initiator());
        maybe_qsy_on_interference(true, &mut rs2, None, &ev_tx, &active_mode, &mut engine2).await;
        assert!(
            !engine2.in_band_interferers().is_empty(),
            "an in-flight session must leave the hint untouched"
        );
    }

    #[tokio::test]
    async fn repeater_enable_with_prebuilt_spawns_thread_and_disable_joins_it() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        // Inject a pre-built repeater using LoopbackBackend engines.
        let rep_rx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let rep_tx = ModemEngine::new(Box::new(LoopbackBackend::new()));
        let rep_cfg = openpulse_repeater::RepeaterConfig {
            enabled: true,
            mode: "BPSK250".to_string(),
            tx_hang_ms: 0,
            full_duplex: false,
            ..Default::default()
        };
        runtime_state.repeater = Some(openpulse_repeater::CrossBandRepeater::new(
            Box::new(openpulse_radio::NoOpPtt::new()),
            rep_rx,
            rep_tx,
            rep_cfg,
        ));

        apply_command_to_engine(
            &ControlCommand::EnableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(runtime_state.repeater_enabled);
        assert!(
            runtime_state.repeater_thread.is_some(),
            "repeater thread should be spawned"
        );
        match rx.recv().await.expect("expected RepeaterChanged event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }

        // Disable the repeater — sets stop flag, joins the thread.
        apply_command_to_engine(
            &ControlCommand::DisableRepeater,
            &mut engine,
            &active_mode,
            &ev_tx,
            None,
            &mut runtime_state,
        )
        .await;

        assert!(!runtime_state.repeater_enabled);
        assert!(
            runtime_state.repeater_thread.is_none(),
            "thread handle should be cleared after join"
        );
        match rx.recv().await.expect("expected RepeaterChanged event") {
            ControlEvent::RepeaterChanged { enabled } => assert!(!enabled),
            other => panic!("expected RepeaterChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn process_received_bytes_with_qsy_req_creates_responder_session() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState {
            local_callsign: "W1AW".into(), // valid MYID so the responder may key up (audit F6)
            ..RuntimeControlState::default()
        };

        // QSY_REQ frame: verb, token, n_candidates
        let qsy_req = b"QSY_REQ tok-resp 2";
        process_received_bytes(
            qsy_req,
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_some(),
            "responder session should be created on first QSY_REQ"
        );
    }

    #[tokio::test]
    async fn process_received_bytes_ignores_non_qsy_text() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        process_received_bytes(
            b"hello world",
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_none(),
            "no session for non-QSY text"
        );
    }

    #[tokio::test]
    async fn process_received_bytes_ignores_non_utf8() {
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState::default();

        process_received_bytes(
            &[0xff, 0xfe, 0x00],
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        assert!(
            runtime_state.qsy_session.is_none(),
            "no session for non-UTF-8 bytes"
        );
    }

    /// End-to-end test: an initiator session generates a QSY_REQ frame; that
    /// frame is fed to the responder path, which must create a session and emit
    /// `QsyIncoming`.
    #[tokio::test]
    async fn qsy_initiator_req_drives_responder_session_and_emits_event() {
        // ── Initiator: build the QSY_REQ frame ─────────────────────────────
        let candidates: Vec<u64> = vec![14074000, 14070000, 7074000];
        let mut init_session = QsySession::new_initiator();
        let actions = init_session
            .initiate(candidates)
            .expect("initiator initiate should succeed");

        // The first action must be SendFrame(Req).
        let req_text = actions
            .iter()
            .find_map(|a| {
                if let QsyAction::SendFrame(frame @ QsyFrame::Req { .. }) = a {
                    Some(encode_qsy_frame(frame))
                } else {
                    None
                }
            })
            .expect("initiator must produce SendFrame(Req) action");

        // Extract the token from the encoded line for assertion.
        let token_from_line = req_text.split_whitespace().nth(1).unwrap().to_string();

        // ── Responder: feed the frame ───────────────────────────────────────
        let mut engine = test_engine();
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".to_string()));
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(32);
        let ev_tx = Arc::new(tx);
        let mut runtime_state = RuntimeControlState {
            local_callsign: "W1AW".into(), // valid MYID so the responder may key up (audit F6)
            ..RuntimeControlState::default()
        };

        process_received_bytes(
            req_text.as_bytes(),
            &mut runtime_state,
            None,
            &ev_tx,
            &active_mode,
            &mut engine,
        )
        .await;

        // Responder must have a session.
        assert!(
            runtime_state.qsy_session.is_some(),
            "responder session should be created on receiving QSY_REQ"
        );

        // A QsyIncoming event must have been broadcast with matching token.
        let ev = rx.recv().await.expect("expected QsyIncoming event");
        match ev {
            ControlEvent::QsyIncoming {
                token,
                n_candidates,
            } => {
                assert_eq!(token, token_from_line, "QsyIncoming token must match frame");
                assert_eq!(
                    n_candidates, 3,
                    "n_candidates must match initiator's candidate list length"
                );
            }
            other => panic!("expected QsyIncoming, got {other:?}"),
        }
    }

    #[test]
    fn ptt_watchdog_fires_after_deadline() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        // A zero max-duration makes any armed deadline immediately expired — fully deterministic,
        // and avoids subtracting past the process uptime on freshly booted CI containers.
        let mut state = RuntimeControlState::default();
        state.ptt.set_max_duration(Duration::ZERO);
        state.ptt.arm();
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(fired, "watchdog must fire when deadline is exceeded");
        assert!(!state.ptt.is_keyed(), "PTT deadline must be cleared");
        let ev = rx.try_recv().expect("PttChanged event must be emitted");
        assert!(
            matches!(ev, ControlEvent::PttChanged { active: false }),
            "event must be PttChanged {{active: false}}"
        );
    }

    #[test]
    fn ptt_watchdog_silent_before_deadline() {
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut state = RuntimeControlState::default();
        state.ptt.arm();
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(!fired, "watchdog must not fire before deadline");
        assert!(state.ptt.is_keyed(), "PTT deadline must remain armed");
    }

    #[test]
    fn ptt_watchdog_silent_when_ptt_not_active() {
        let (tx, _) = broadcast::channel::<ControlEvent>(16);
        let ev_tx = Arc::new(tx);
        let mut state = RuntimeControlState::default();
        let fired = check_ptt_watchdog(&mut state, &ev_tx);
        assert!(!fired, "watchdog must not fire when PTT is not active");
    }

    #[test]
    fn rendezvous_token_is_two_base36_chars() {
        let t = rendezvous_token(1_700_000_123_456);
        assert_eq!(t.len(), 2);
        assert!(t
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
    }

    /// A discovery runtime in `tx_mode` with `callsign`, dwelling on the 20 m calling channel.
    fn discovery_rs(tx_mode: openpulse_discovery::TxMode, callsign: &str) -> RuntimeControlState {
        use openpulse_discovery::{DiscoveryParams, DiscoveryRuntime, Submode};
        RuntimeControlState {
            discovery_rendezvous_channels_hz: [(
                "20m".to_string(),
                vec![14_101_000, 14_103_000, 14_105_000],
            )]
            .into_iter()
            .collect(),
            discovery: Some(DiscoveryRuntime::new(DiscoveryParams {
                enabled: true,
                idle_grace_ms: 0,
                dwell_ms: 0,
                station_ttl_ms: 3_600_000,
                submode: Submode::Normal,
                calling_freq_hz: 14_078_000, // 20 m
                tx_mode,
                callsign: callsign.into(),
                grid: "JN58".into(),
                hint: None,
                heartbeat_interval_slots: 8,
                hint_interval_beacons: 0,
                tx_offset_hz: 1500.0,
                max_clock_skew_ms: 2000,
            })),
            ..RuntimeControlState::default()
        }
    }

    fn drain_command_errors(rx: &mut broadcast::Receiver<ControlEvent>) -> Vec<String> {
        let mut errs = Vec::new();
        while let Ok(e) = rx.try_recv() {
            if let ControlEvent::CommandError { command, .. } = e {
                errs.push(command);
            }
        }
        errs
    }

    #[tokio::test]
    async fn dispatch_rejects_unknown_mode_without_mutating_state() {
        // Audit #14: a bad SetMode must be rejected before it writes active_mode, so a typo can't
        // silently deafen RX/station-ID while the client is told "ok".
        let (cmd_tx, _cmd_rx) = mpsc::channel::<ControlCommand>(4);
        let active_mode: SharedMode = Arc::new(Mutex::new("BPSK250".into()));
        let atten: SharedAttenuation = Arc::new(Mutex::new(0.0));
        let qsy: SharedQsyEnabled = Arc::new(Mutex::new(false));
        let bp: SharedBandplanMode = Arc::new(Mutex::new("ham_iaru_region1".into()));
        let tuner: SharedTunerOnHighSWR = Arc::new(Mutex::new(false));
        let valid: ValidModes = Arc::new(["BPSK250".to_string(), "QPSK500".to_string()].into());

        // Unknown mode → error response, state untouched, command NOT forwarded.
        let resp = dispatch_command(
            &ControlCommand::SetMode {
                mode: "NONSENSE999".into(),
            },
            &cmd_tx,
            &active_mode,
            &atten,
            &qsy,
            &bp,
            &tuner,
            &valid,
        )
        .await;
        assert!(!resp.ok, "unknown mode must be rejected: {resp:?}");
        assert_eq!(*active_mode.lock().await, "BPSK250", "state unchanged");

        // Valid mode → applied.
        let resp = dispatch_command(
            &ControlCommand::SetMode {
                mode: "QPSK500".into(),
            },
            &cmd_tx,
            &active_mode,
            &atten,
            &qsy,
            &bp,
            &tuner,
            &valid,
        )
        .await;
        assert!(resp.ok, "valid mode accepted: {resp:?}");
        assert_eq!(*active_mode.lock().await, "QPSK500");
    }

    #[test]
    fn rendezvous_with_starts_a_proposal_when_configured() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = discovery_rs(openpulse_discovery::TxMode::Full, "DC0SK");
        start_rendezvous_cmd("KN4CRD", &mut rs, &ev, 1_700_000_000_000);
        assert!(
            rs.discovery.as_ref().unwrap().rendezvous_active(),
            "a proposal is in flight"
        );
        assert!(
            drain_command_errors(&mut rx).is_empty(),
            "no error on the happy path"
        );
    }

    #[test]
    fn rendezvous_with_errors_when_discovery_is_unconfigured() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState::default(); // discovery: None
        start_rendezvous_cmd("KN4CRD", &mut rs, &ev, 1_700_000_000_000);
        assert_eq!(drain_command_errors(&mut rx), vec!["rendezvous_with"]);
    }

    #[test]
    fn rendezvous_with_errors_without_channels_for_the_band() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = discovery_rs(openpulse_discovery::TxMode::Full, "DC0SK");
        rs.discovery_rendezvous_channels_hz.clear(); // no table for 20 m
        start_rendezvous_cmd("KN4CRD", &mut rs, &ev, 1_700_000_000_000);
        assert_eq!(drain_command_errors(&mut rx), vec!["rendezvous_with"]);
        assert!(!rs.discovery.as_ref().unwrap().rendezvous_active());
    }

    #[test]
    fn rendezvous_with_errors_without_a_callsign() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = discovery_rs(openpulse_discovery::TxMode::Full, ""); // no callsign → TX gated
        start_rendezvous_cmd("KN4CRD", &mut rs, &ev, 1_700_000_000_000);
        assert_eq!(drain_command_errors(&mut rx), vec!["rendezvous_with"]);
        assert!(!rs.discovery.as_ref().unwrap().rendezvous_active());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod handshake_rf_tests {
    use super::*;
    use bpsk_plugin::BpskPlugin;
    use openpulse_audio::LoopbackBackend;
    use openpulse_core::compression::CompressionAlgorithm;
    use openpulse_core::fec::FecMode;
    use tokio::sync::Mutex;

    fn bpsk_engine() -> ModemEngine {
        let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
        e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        e
    }

    fn mode() -> SharedMode {
        Arc::new(Mutex::new("BPSK250".to_string()))
    }

    /// The responder reassembles a fragmented, signed CONREQ from RF, records the proven peer
    /// identity (callsign + grid + pubkey), and emits `PeerVerified`.
    #[tokio::test]
    async fn responder_verifies_conreq_fragments_and_records_peer() {
        let conreq = ConReq::create_with_grid(
            "W1AW",
            &[1u8; 32],
            vec![SigningMode::Normal],
            "W1AW-1700000000000",
            vec![],
            vec![],
            "FN31pr",
        )
        .unwrap();
        let frags = sar_encode(0, &conreq.encode().unwrap()).unwrap();
        assert!(frags.len() > 1, "a CONREQ exceeds one modem frame");

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState {
            local_callsign: "K2XYZ".into(),
            local_grid: "EM69".into(),
            station_seed: [2u8; 32],
            ..RuntimeControlState::default()
        };

        for (i, frag) in frags.iter().enumerate() {
            process_received_bytes(frag, &mut rs, None, &ev, &mode, &mut eng).await;
            if i + 1 < frags.len() {
                assert!(
                    rs.verified_peer.is_none(),
                    "must not verify before all fragments arrive"
                );
            }
        }

        let vp = rs
            .verified_peer
            .expect("peer verified after final fragment");
        assert_eq!(vp.callsign, "W1AW");
        assert_eq!(vp.grid, "FN31pr");
        assert_eq!(vp.pubkey, conreq.pubkey);
        assert!(
            matches!(rx.try_recv(), Ok(ControlEvent::PeerVerified { callsign, grid })
                if callsign == "W1AW" && grid == "FN31pr"),
            "PeerVerified event should be emitted"
        );
    }

    /// Audit F6/F4 unit coverage: `local_callsign_valid` rejects the empty/`N0CALL` sentinels, and
    /// `rf_peer_trust` maps a verified peer to its over-air trust (never `Verified` over RF).
    #[test]
    fn callsign_validity_and_rf_peer_trust() {
        let mut rs = RuntimeControlState::default();
        rs.local_callsign = String::new();
        assert!(!rs.local_callsign_valid(), "empty callsign is not valid");
        rs.local_callsign = "n0call".into();
        assert!(!rs.local_callsign_valid(), "N0CALL sentinel is not valid");
        rs.local_callsign = "W1AW".into();
        assert!(rs.local_callsign_valid(), "a real callsign is valid");

        // No verified peer this session → Unverified.
        assert_eq!(rs.rf_peer_trust(), ConnectionTrustLevel::Unverified);

        rs.verified_peer = Some(VerifiedPeer {
            callsign: "K2XYZ".into(),
            grid: "EM69".into(),
            pubkey: vec![2u8; 32],
            profile_compatible: None,
        });
        // First-seen (unknown) key over air → Low.
        assert_eq!(rs.rf_peer_trust(), ConnectionTrustLevel::Low);

        // A trust-store (Full) key, but still over-air without PSK → Reduced, never Verified.
        rs.trust_store.add_trusted("K2XYZ", [2u8; 32]);
        assert_eq!(rs.rf_peer_trust(), ConnectionTrustLevel::Reduced);
    }

    /// Audit F6 (§97.119): a responder with no valid callsign that hears a full CONREQ must not key
    /// the transmitter to answer with a CONACK, and must not record a half-handshake the peer never
    /// sees completed.
    #[tokio::test]
    async fn responder_without_callsign_does_not_transmit_conack() {
        let conreq = ConReq::create_with_grid(
            "W1AW",
            &[1u8; 32],
            vec![SigningMode::Normal],
            "W1AW-1700000000000",
            vec![],
            vec![],
            "FN31pr",
        )
        .unwrap();
        let frags = sar_encode(0, &conreq.encode().unwrap()).unwrap();

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState {
            local_callsign: String::new(), // no MYID
            station_seed: [2u8; 32],
            ..RuntimeControlState::default()
        };

        let before = eng.frames_transmitted();
        for frag in &frags {
            process_received_bytes(frag, &mut rs, None, &ev, &mode, &mut eng).await;
        }
        assert_eq!(
            eng.frames_transmitted(),
            before,
            "no CONACK may be transmitted without a valid callsign"
        );
        assert!(
            rs.verified_peer.is_none(),
            "a half-handshake must not be recorded when we cannot reply"
        );
    }

    /// Audit F6 (§97.119): a responder with no valid callsign that hears a QSY_REQ must not engage
    /// the QSY responder (which would key the transmitter for a reply, even a Reject).
    #[tokio::test]
    async fn responder_without_callsign_ignores_qsy_req() {
        let req = encode_qsy_frame(&QsyFrame::Req {
            token: "TOK123".into(),
            n_candidates: 3,
        });

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState {
            local_callsign: "N0CALL".into(),
            ..RuntimeControlState::default()
        };

        let before = eng.frames_transmitted();
        process_received_bytes(req.as_bytes(), &mut rs, None, &ev, &mode, &mut eng).await;
        assert_eq!(
            eng.frames_transmitted(),
            before,
            "no QSY reply may be transmitted without a valid callsign"
        );
        assert!(
            rs.qsy_session.is_none(),
            "no QSY responder session may be created without a valid callsign"
        );
    }

    /// The initiator verifies the peer's CONACK against its in-flight CONREQ, records the verified
    /// peer, clears the pending handshake, and stamps the verified grid onto the logbook QSO.
    #[tokio::test]
    async fn initiator_verifies_conack_and_stamps_logbook_grid() {
        let tmp = std::env::temp_dir().join(format!("ophs-init-{}.adi", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        let mut rs = RuntimeControlState {
            local_callsign: "W1AW".into(),
            local_grid: "FN31".into(),
            station_seed: [1u8; 32],
            pending_handshake: Some(PendingHandshake {
                session_id: "W1AW-1".into(),
                peer_callsign: "K2XYZ".into(),
                started_at: Instant::now(),
            }),
            ..RuntimeControlState::default()
        };
        rs.logbook = crate::logbook::Logbook::new(
            true,
            tmp.to_str().unwrap(),
            "W1AW",
            "FN31",
            &Default::default(),
        );
        rs.logbook
            .begin_qso("K2XYZ", "BPSK250", Some(14_070_000), 1_700_000_000_000);

        let conack = ConAck::create_with_grid(
            "K2XYZ",
            &[2u8; 32],
            SigningMode::Normal,
            "W1AW-1",
            CompressionAlgorithm::None,
            FecMode::None,
            "EM69",
        )
        .unwrap();
        let frags = sar_encode(0, &conack.encode().unwrap()).unwrap();

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        for frag in &frags {
            process_received_bytes(frag, &mut rs, None, &ev, &mode, &mut eng).await;
        }

        let vp = rs.verified_peer.clone().expect("verified");
        assert_eq!(vp.callsign, "K2XYZ");
        assert_eq!(vp.grid, "EM69");
        assert!(
            rs.pending_handshake.is_none(),
            "pending handshake cleared on verified CONACK"
        );

        // The verified on-air grid (EM69) must land in the ADIF QSO record.
        rs.logbook.end_qso(1_700_000_300_000, Some(10.0)).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        assert!(
            body.contains("<GRIDSQUARE:4>EM69"),
            "ADIF should carry the verified grid; got: {body}"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// A CONACK whose session id doesn't match the in-flight CONREQ is ignored: no peer is
    /// recorded and the pending handshake is preserved (so the real CONACK can still complete it).
    #[tokio::test]
    async fn conack_with_mismatched_session_is_ignored() {
        let mut rs = RuntimeControlState {
            local_callsign: "W1AW".into(),
            station_seed: [1u8; 32],
            pending_handshake: Some(PendingHandshake {
                session_id: "W1AW-1".into(),
                peer_callsign: "K2XYZ".into(),
                started_at: Instant::now(),
            }),
            ..RuntimeControlState::default()
        };
        let conack = ConAck::create(
            "K2XYZ",
            &[2u8; 32],
            SigningMode::Normal,
            "SOME-OTHER-SESSION",
            CompressionAlgorithm::None,
            FecMode::None,
        )
        .unwrap();
        let frags = sar_encode(0, &conack.encode().unwrap()).unwrap();

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        for frag in &frags {
            process_received_bytes(frag, &mut rs, None, &ev, &mode, &mut eng).await;
        }
        assert!(
            rs.verified_peer.is_none(),
            "mismatched session must not verify"
        );
        assert!(
            rs.pending_handshake.is_some(),
            "mismatched CONACK must not clear the pending handshake"
        );
    }

    /// Audit F2: a CONACK that echoes the correct (cleartext, guessable) session id but comes from a
    /// station other than the one we dialed is ignored — an attacker cannot race a self-signed CONACK
    /// under their own callsign and be recorded as the dialed peer. The pending handshake is preserved.
    #[tokio::test]
    async fn conack_from_undialed_station_is_ignored() {
        let mut rs = RuntimeControlState {
            local_callsign: "W1AW".into(),
            station_seed: [1u8; 32],
            pending_handshake: Some(PendingHandshake {
                session_id: "W1AW-1".into(),
                peer_callsign: "K2XYZ".into(),
                started_at: Instant::now(),
            }),
            ..RuntimeControlState::default()
        };
        // Attacker "N0EVL" signs a CONACK with its own key, correctly echoing the session id.
        let conack = ConAck::create(
            "N0EVL",
            &[9u8; 32],
            SigningMode::Normal,
            "W1AW-1",
            CompressionAlgorithm::None,
            FecMode::None,
        )
        .unwrap();
        let frags = sar_encode(0, &conack.encode().unwrap()).unwrap();

        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        for frag in &frags {
            process_received_bytes(frag, &mut rs, None, &ev, &mode, &mut eng).await;
        }
        assert!(
            rs.verified_peer.is_none(),
            "CONACK from an undialed station must not verify"
        );
        assert!(
            rs.pending_handshake.is_some(),
            "CONACK from an undialed station must not clear the pending handshake"
        );
    }

    /// `ConnectPeer` initiates the signed handshake: it records a pending handshake keyed on a
    /// session id derived from the local callsign.
    #[tokio::test]
    async fn connect_peer_initiates_signed_handshake() {
        let mut eng = bpsk_engine();
        let mode = mode();
        let (tx, _rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState {
            local_callsign: "W1AW".into(),
            local_grid: "FN31".into(),
            station_seed: [1u8; 32],
            ..RuntimeControlState::default()
        };
        apply_command_to_engine(
            &ControlCommand::ConnectPeer {
                callsign: "K2XYZ".into(),
            },
            &mut eng,
            &mode,
            &ev,
            None,
            &mut rs,
        )
        .await;
        let p = rs
            .pending_handshake
            .expect("pending handshake set by ConnectPeer");
        assert_eq!(p.peer_callsign, "K2XYZ");
        assert!(p.session_id.starts_with("W1AW-"));
    }

    /// A CONREQ SAR fragment (a full 255-byte modem frame) survives a real BPSK250 round trip.
    #[test]
    fn max_size_handshake_fragment_survives_bpsk_round_trip() {
        use openpulse_modem::channel_sim::ChannelSimHarness;
        let mut h = ChannelSimHarness::new();
        h.tx_engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .unwrap();
        h.rx_engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .unwrap();
        let conreq = ConReq::create_with_grid(
            "W1AW",
            &[1u8; 32],
            vec![SigningMode::Normal],
            "W1AW-1700000000000",
            vec![],
            vec![],
            "FN31pr",
        )
        .unwrap();
        let frag = sar_encode(0, &conreq.encode().unwrap()).unwrap().remove(0);
        assert_eq!(
            frag.len(),
            255,
            "first fragment should be a full modem frame"
        );
        h.tx_engine.transmit(&frag, "BPSK250", None).unwrap();
        h.route_clean();
        let rx = h.rx_engine.receive("BPSK250", None).unwrap_or_default();
        assert_eq!(
            rx, frag,
            "a CONREQ SAR fragment must survive BPSK250 transport"
        );
    }

    /// An unanswered CONREQ is abandoned after the timeout, emitting a `CommandError`.
    #[test]
    fn pending_handshake_expires_after_timeout() {
        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);
        let mut rs = RuntimeControlState {
            pending_handshake: Some(PendingHandshake {
                session_id: "s".into(),
                peer_callsign: "K2XYZ".into(),
                started_at: Instant::now() - HANDSHAKE_TIMEOUT - Duration::from_secs(1),
            }),
            ..RuntimeControlState::default()
        };
        expire_pending_handshake(&mut rs, &ev);
        assert!(
            rs.pending_handshake.is_none(),
            "stale handshake must be dropped"
        );
        assert!(
            matches!(rx.try_recv(), Ok(ControlEvent::CommandError { command, .. })
                if command == "connect_peer"),
            "expiry should emit a connect_peer CommandError"
        );
    }

    #[test]
    fn discovery_commands_toggle_the_runtime_and_list_stations() {
        use openpulse_discovery::{DiscoveryParams, DiscoveryRuntime, Submode};

        let (tx, mut rx) = broadcast::channel::<ControlEvent>(16);
        let ev = Arc::new(tx);

        // Unconfigured: EnableDiscovery reports an error.
        let mut rs = RuntimeControlState::default();
        set_discovery_enabled(true, &mut rs, &ev);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::CommandError { command, .. }) if command == "enable_discovery"
        ));

        // Configured: EnableDiscovery emits DiscoveryStatus; ListStations emits an (empty) StationList.
        rs.discovery = Some(DiscoveryRuntime::new(DiscoveryParams {
            enabled: false,
            idle_grace_ms: 0,
            dwell_ms: 0,
            station_ttl_ms: 3_600_000,
            submode: Submode::Normal,
            calling_freq_hz: 14_078_000,
            tx_mode: openpulse_discovery::TxMode::RxOnly,
            callsign: String::new(),
            grid: String::new(),
            hint: None,
            heartbeat_interval_slots: 8,
            hint_interval_beacons: 3,
            tx_offset_hz: 1500.0,
            max_clock_skew_ms: 2000,
        }));
        set_discovery_enabled(true, &mut rs, &ev);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::DiscoveryStatus { dial_freq_hz, .. }) if dial_freq_hz == 14_078_000
        ));
        emit_station_list(&rs, &ev);
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::StationList { stations }) if stations.is_empty()
        ));
    }
}
