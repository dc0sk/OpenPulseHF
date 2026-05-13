//! Memory-ARQ soft combining integration test for SC-FDMA.
//!
//! Verifies that combining multiple noisy retransmissions of the same SCFDMA52
//! frame recovers payload bytes in a regime where a single noisy copy fails.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::fec::SoftCombiner;
use openpulse_modem::engine::ModemEngine;
use scfdma_plugin::ScFdmaPlugin;

fn modem_pair_with_scfdma() -> (ModemEngine, LoopbackBackend, ModemEngine, LoopbackBackend) {
    let tx_backend = LoopbackBackend::new();
    let rx_backend = LoopbackBackend::new();

    let mut tx = ModemEngine::new(Box::new(tx_backend.clone_shared()));
    let mut rx = ModemEngine::new(Box::new(rx_backend.clone_shared()));

    tx.register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("TX plugin registration");
    rx.register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("RX plugin registration");

    (tx, tx_backend, rx, rx_backend)
}

#[test]
fn scfdma52_memory_arq_soft_combining_recovers_payload() {
    let (mut tx, tx_backend, mut rx, rx_backend) = modem_pair_with_scfdma();
    let payload = b"SCFDMA52 memory-ARQ soft combining test payload";

    // Encode one RS-protected frame once.
    tx.transmit_with_fec(payload, "SCFDMA52", None)
        .expect("TX with FEC");
    let tx_samples = tx_backend.drain_samples();
    assert!(!tx_samples.is_empty(), "TX samples should not be empty");

    // Find a deterministic operating point where one-shot fails but combining succeeds.
    let mut found = false;
    for snr_db in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        let mut noisy_single = AwgnChannel::new(AwgnConfig::new(snr_db, Some(101))).expect("awgn");
        let one_shot = noisy_single.apply(&tx_samples);
        rx_backend.fill_samples(&one_shot);
        let single_failed = rx.receive_with_soft_combining("SCFDMA52", None, 1).is_err();

        // Combine 8 independently-noised retransmissions of the same signal.
        let mut combiner = SoftCombiner::new();
        for seed in [201_u64, 202, 203, 204, 205, 206, 207, 208] {
            let mut channel = AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed))).expect("awgn");
            let noisy = channel.apply(&tx_samples);
            combiner.push(&noisy);
        }

        rx_backend.fill_samples(&combiner.combine());
        let combined_ok = rx
            .receive_with_soft_combining("SCFDMA52", None, 1)
            .map(|b| b == payload)
            .unwrap_or(false);

        if single_failed && combined_ok {
            found = true;
            break;
        }
    }

    assert!(
        found,
        "could not find a fixed SNR point (0..5 dB) where one-shot fails and combined decode succeeds"
    );
}
