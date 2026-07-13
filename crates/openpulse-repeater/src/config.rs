/// Configuration for the cross-band repeater.
#[derive(Debug, Clone)]
pub struct RepeaterConfig {
    /// Enable the repeater; `relay_one_frame()` is a no-op when `false`.
    pub enabled: bool,
    /// Modulation mode string used for both RX and TX (e.g. `"BPSK250"`).
    pub mode: String,
    /// Milliseconds to hold PTT after the last TX byte (half-duplex only).
    pub tx_hang_ms: u64,
    /// When true, PTT is held for the entire relay session by `run_full_duplex()`.
    /// `tx_hang_ms` is ignored in full-duplex mode.
    pub full_duplex: bool,
    /// Station callsign transmitted for §97.119 identification of the *transmitting* rig (rig_b). Empty
    /// disables auto-ID (the repeater then never keys an ID — the operator is responsible).
    pub callsign: String,
    /// Auto-ID interval in seconds (Part-97 §97.119 = 600 = 10 min). `0` disables auto-ID.
    pub id_interval_secs: u64,
}

impl Default for RepeaterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "BPSK250".into(),
            tx_hang_ms: 0,
            full_duplex: false,
            callsign: String::new(),
            id_interval_secs: 600,
        }
    }
}
