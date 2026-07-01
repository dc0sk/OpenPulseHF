//! Regression: CE-SSB must NOT touch the dense-constellation rungs (≥16QAM), on
//! either OFDM or SC-FDMA. Its envelope clip injects EVM that breaks acquisition /
//! decode on tight constellations — exactly the hpx_hf / OFDM top modes the rate
//! ladder climbs to. With CE-SSB wrongly enabled there, the linksim throughput at
//! high SNR regressed (~40→30 kbit/s) because SL10/SL11 frames stopped decoding.
//! See `ModemEngine::cessb_benefits`.
use openpulse_channel::{build_channel, AwgnConfig, ChannelModelConfig};
use openpulse_core::plugin::ModulationPlugin;
use openpulse_modem::channel_sim::ChannelSimHarness;

fn decoded_frames<P, F>(
    cessb: bool,
    make: F,
    soft: bool,
    mode: &str,
    snr: f32,
    frames: usize,
) -> usize
where
    P: ModulationPlugin + 'static,
    F: Fn() -> P,
{
    let mut h = ChannelSimHarness::new();
    let _ = h.tx_engine.register_plugin(Box::new(make()));
    let _ = h.rx_engine.register_plugin(Box::new(make()));
    h.tx_engine.set_cessb_enabled(cessb);
    let mut ch = build_channel(
        &ChannelModelConfig::Awgn(AwgnConfig {
            snr_db: snr,
            seed: Some(7),
        }),
        Some(7),
    )
    .unwrap();
    let mut ok = 0;
    for f in 0..frames {
        let mut payload = format!("frame {f}; ").into_bytes();
        while payload.len() < 200 {
            payload.extend_from_slice(b"the quick brown fox jumps over the lazy dog. ");
        }
        payload.truncate(200);
        let tx = if soft {
            h.tx_engine
                .transmit_with_soft_viterbi_fec(&payload, mode, None)
        } else {
            h.tx_engine.transmit_with_fec(&payload, mode, None)
        };
        if tx.is_err() {
            continue;
        }
        h.route_tapped(&mut *ch);
        let rx = if soft {
            h.rx_engine.receive_with_soft_viterbi_fec(mode, None)
        } else {
            h.rx_engine.receive_with_fec(mode, None)
        };
        if rx.map(|d| d == payload).unwrap_or(false) {
            ok += 1;
        }
    }
    ok
}

/// CE-SSB default (on) must be a no-op on the dense SC-FDMA rung — i.e.
/// `cessb_benefits("SCFDMA…")` is false — so it decodes as well as with CE-SSB off.
#[test]
fn cessb_default_does_not_break_scfdma_64qam() {
    use scfdma_plugin::ScFdmaPlugin;
    let n = 16;
    let on = decoded_frames(true, ScFdmaPlugin::new, false, "SCFDMA52-64QAM", 35.0, n);
    let off = decoded_frames(false, ScFdmaPlugin::new, false, "SCFDMA52-64QAM", 35.0, n);
    assert_eq!(
        off, n,
        "baseline: SCFDMA52-64QAM should fully decode at 35 dB"
    );
    assert_eq!(
        on, off,
        "CE-SSB default must be a no-op on SC-FDMA (got {on}/{n} on vs {off}/{n} off)"
    );
}

/// CE-SSB default (on) must be a no-op on the dense OFDM rungs (≥16QAM): the gate
/// excludes them, so they decode as well as with CE-SSB off. 16QAM is the marginal
/// rung — it survives easy AWGN but breaks on fading, so it is excluded too.
#[test]
fn cessb_default_does_not_break_dense_ofdm() {
    use ofdm_plugin::OfdmPlugin;
    let n = 12;
    for (mode, snr) in [
        ("OFDM52-16QAM", 22.0f32),
        ("OFDM52-32QAM", 26.0),
        ("OFDM52-64QAM", 30.0),
    ] {
        let on = decoded_frames(true, OfdmPlugin::new, true, mode, snr, n);
        let off = decoded_frames(false, OfdmPlugin::new, true, mode, snr, n);
        assert_eq!(off, n, "baseline: {mode} should fully decode at {snr} dB");
        assert_eq!(
            on, off,
            "CE-SSB default must be a no-op on {mode} (got {on}/{n} on vs {off}/{n} off)"
        );
    }
}

/// CE-SSB default (on) must be a no-op on the OFDM 8PSK rung — the gate excludes it
/// (its ±22.5° margins can't absorb the clip EVM), so it decodes as if CE-SSB were off.
#[test]
fn cessb_default_keeps_low_order_ofdm_decoding() {
    use ofdm_plugin::OfdmPlugin;
    let n = 12;
    let on = decoded_frames(true, OfdmPlugin::new, true, "OFDM52-8PSK", 18.0, n);
    assert_eq!(
        on, n,
        "OFDM52-8PSK must still decode with CE-SSB on (got {on}/{n})"
    );
}
