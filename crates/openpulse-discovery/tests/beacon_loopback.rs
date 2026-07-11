//! Full TX→RX loopback: an @OPULSE hint beacon built by the TX side transmits, is decoded off the
//! air, and is recognised as an OpenPulse peer by the RX HintAssembler — proving Phase E TX and the
//! Phase C/D RX close the loop OpenPulse-to-OpenPulse.

use js8_plugin::beacon::{frame_audio, opulse_hint};
use js8_plugin::decoder::{decode_window, DecodeCfg};
use js8_plugin::submode::{params, Submode};
use openpulse_discovery::{encode_hint, HintAssembler, HintPayload};

#[test]
fn opulse_beacon_transmits_and_is_recognized_end_to_end() {
    let payload = HintPayload {
        caps: 0xB105,
        pref_channel: 42,
        listen_submode: 1,
    };
    let text = encode_hint(&payload, "DC0SK");
    let frames = opulse_hint("DC0SK", "JN58", &text);

    let sm = params(Submode::Normal);
    let cfg = DecodeCfg {
        base_min: 1490.0,
        base_max: 1510.0,
        ..DecodeCfg::default()
    };
    let mut asm = HintAssembler::new(6.0, 8);
    let mut recognized = None;
    for (slot, f) in frames.iter().enumerate() {
        let audio = frame_audio(f, 1500.0, Submode::Normal);
        for d in decode_window(&audio, &sm, &cfg) {
            if let Some(r) = asm.ingest(&d.payload, d.i3bit, d.base_freq_hz, slot as u64) {
                recognized = Some(r);
            }
        }
    }

    let r = recognized.expect("the beacon must be recognized end to end");
    assert_eq!(r.callsign, "DC0SK");
    assert_eq!(r.grid.as_deref(), Some("JN58"));
    assert_eq!(r.hint.caps, 0xB105);
    assert_eq!(r.hint.pref_channel, 42);
}
