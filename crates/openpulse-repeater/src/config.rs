/// Configuration for the cross-band repeater.
#[derive(Debug, Clone)]
pub struct RepeaterConfig {
    /// Enable the repeater; `relay_one_frame()` is a no-op when `false`.
    pub enabled: bool,
    /// Modulation mode string used for both RX and TX (e.g. `"BPSK250"`).
    pub mode: String,
    /// Milliseconds to hold PTT after the last TX byte.
    pub tx_hang_ms: u64,
}

impl Default for RepeaterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "BPSK250".into(),
            tx_hang_ms: 0,
        }
    }
}
