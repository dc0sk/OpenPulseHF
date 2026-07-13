//! End-to-end rendezvous across two independent `DiscoveryRuntime`s (FF-15 Phase F, acceptance).
//!
//! Rather than a real-time twin daemon (JS8's 15 s UTC slots would make the exchange minutes long),
//! this shuttles the *actual* GFSK audio each runtime transmits into the other's capture buffer under a
//! manual clock: the initiator's `Propose` over is decoded by the responder, whose `Accept` over is
//! decoded by the initiator — both reaching `RendezvousAgreed` on the same channel index. It exercises
//! the full stack (initiator session ↔ directed TX framing ↔ GFSK modem ↔ RX reassembler ↔ responder).

use openpulse_discovery::{
    DiscoveryOutcome, DiscoveryParams, DiscoveryRuntime, DiscoveryState, Submode, TxMode,
};

/// A dwelling Full-mode runtime for `callsign`, with heartbeat cadence pushed out so only rendezvous
/// frames transmit, and the given rendezvous channel indices available.
fn station(callsign: &str, channels: Vec<u8>) -> DiscoveryRuntime {
    let mut rt = DiscoveryRuntime::new(DiscoveryParams {
        enabled: true,
        idle_grace_ms: 0,
        dwell_ms: 0,
        station_ttl_ms: 3_600_000,
        submode: Submode::Normal,
        calling_freq_hz: 14_078_000,
        tx_mode: TxMode::Full,
        callsign: callsign.into(),
        grid: "JN58".into(),
        hint: None,
        heartbeat_interval_slots: 10_000,
        hint_interval_beacons: 0,
        tx_offset_hz: 1500.0,
        max_clock_skew_ms: 2000,
    });
    rt.tick(1000, true); // activate
    rt.qsy_complete(true); // dwell
    rt.set_rendezvous_channels(channels);
    rt
}

/// Tick `rt` across slots, collecting every transmitted-frame audio buffer until its TX queue drains,
/// plus the non-TX outcomes seen along the way (e.g. a responder's `RendezvousAgreed`, which surfaces
/// only once its Accept over has fully transmitted).
fn drain_tx(rt: &mut DiscoveryRuntime, t: &mut u64) -> (Vec<Vec<f32>>, Vec<DiscoveryOutcome>) {
    let mut frames = Vec::new();
    let mut outcomes = Vec::new();
    for _ in 0..16 {
        *t += 15_000;
        let mut got = false;
        for o in rt.tick(*t, true) {
            if let DiscoveryOutcome::TransmitBeacon { audio, .. } = o {
                frames.push(audio);
                got = true;
            } else {
                outcomes.push(o);
            }
        }
        if !got && !frames.is_empty() {
            break; // queue drained
        }
    }
    (frames, outcomes)
}

/// Deliver each transmitted-frame audio buffer into `rt`'s capture, crossing a slot boundary per frame
/// to force the decode, and collect the outcomes.
fn deliver(rt: &mut DiscoveryRuntime, frames: &[Vec<f32>], t: &mut u64) -> Vec<DiscoveryOutcome> {
    let mut out = Vec::new();
    for audio in frames {
        rt.push_audio(audio);
        rt.tick(*t, true); // establish the slot
        *t += 15_000;
        out.extend(rt.tick(*t, true)); // cross the boundary → decode
    }
    out
}

fn agreed_channel(outcomes: &[DiscoveryOutcome], peer: &str) -> Option<u8> {
    outcomes.iter().find_map(|o| match o {
        DiscoveryOutcome::RendezvousAgreed {
            peer: p, channel, ..
        } if p == peer => Some(*channel),
        _ => None,
    })
}

#[test]
fn two_runtimes_reach_a_rendezvous_over_the_air() {
    let mut initiator = station("DC0SK", vec![0, 1, 2]);
    let mut responder = station("KN4CRD", vec![0, 1, 2]);
    assert_eq!(initiator.state(), DiscoveryState::Dwelling);
    assert_eq!(responder.state(), DiscoveryState::Dwelling);

    let mut t = 1000u64;

    // DC0SK proposes channels 1 then 0 to KN4CRD.
    initiator.start_rendezvous("KN4CRD", "R7", vec![1, 0], 50);
    let (propose_audio, _) = drain_tx(&mut initiator, &mut t);
    assert!(!propose_audio.is_empty(), "the Propose over transmitted");

    // The responder decodes the Propose and enqueues its Accept, but withholds the agreement until the
    // Accept is on the air (audit #4b) — so nothing is agreed during receive.
    let resp_recv = deliver(&mut responder, &propose_audio, &mut t);
    assert_eq!(
        agreed_channel(&resp_recv, "DC0SK"),
        None,
        "responder defers agreement until its Accept is sent"
    );

    // The responder's Accept over transmits; the agreement surfaces once it has, on the highest-ranked
    // common channel (1). The initiator then decodes the Accept and reaches the same agreement.
    let (accept_audio, resp_out) = drain_tx(&mut responder, &mut t);
    assert!(!accept_audio.is_empty(), "the Accept over transmitted");
    assert_eq!(
        agreed_channel(&resp_out, "DC0SK"),
        Some(1),
        "responder agreed on channel 1 after sending the Accept: {resp_out:?}"
    );
    let init_out = deliver(&mut initiator, &accept_audio, &mut t);
    assert_eq!(
        agreed_channel(&init_out, "KN4CRD"),
        Some(1),
        "initiator agreed on channel 1: {init_out:?}"
    );
    assert!(
        !initiator.rendezvous_active(),
        "the initiator session concluded"
    );
}

#[test]
fn two_runtimes_with_no_common_channel_do_not_agree() {
    let mut initiator = station("DC0SK", vec![0, 1, 2]);
    let mut responder = station("KN4CRD", vec![5, 6]); // disjoint from the proposal
    let mut t = 1000u64;

    initiator.start_rendezvous("KN4CRD", "R7", vec![1, 0], 50);
    let (propose_audio, _) = drain_tx(&mut initiator, &mut t);
    let resp_out = deliver(&mut responder, &propose_audio, &mut t);
    assert_eq!(
        agreed_channel(&resp_out, "DC0SK"),
        None,
        "no agreement without a common channel"
    );

    // The responder still sends a Reject over; the initiator surfaces it as rejected, not agreed.
    let (reject_audio, resp_out) = drain_tx(&mut responder, &mut t);
    assert!(
        !resp_out
            .iter()
            .any(|o| matches!(o, DiscoveryOutcome::RendezvousAgreed { .. })),
        "responder never agrees with no common channel"
    );
    let init_out = deliver(&mut initiator, &reject_audio, &mut t);
    assert!(
        agreed_channel(&init_out, "KN4CRD").is_none(),
        "initiator must not agree"
    );
    assert!(
        init_out
            .iter()
            .any(|o| matches!(o, DiscoveryOutcome::RendezvousRejected { .. })),
        "initiator saw the reject: {init_out:?}"
    );
}
