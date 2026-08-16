//! #1125 gate — a consumer that laps the event ring must RESUME, not freeze.
//!
//! `tokio::sync::broadcast` returns `Err(RecvError::Lagged(n))` when a subscriber falls behind the
//! engine's 64-slot ring. A drain written as `while let Ok(ev) = rx.try_recv()` treats that exactly
//! like "no more events" and stops forever, so one overflow silently freezes that consumer's view.
//!
//! The daemon's engine→`ControlEvent` forwarder is the consumer every control client, the WebSocket
//! bridge and the audit recorder sit behind, so it is the one gate worth having: if it stops, every
//! downstream view stops with it.
//!
//! The overflow is forced rather than hoped for — `#[tokio::test]` runs a current-thread runtime, so
//! the forwarder task cannot be scheduled while the flood loop below is running (it contains no
//! await point). The test asserts the lap actually happened before it asserts recovery; without that
//! tripwire a forwarder that simply kept up would pass this vacuously.

use std::net::SocketAddr;
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_daemon::protocol::ControlEvent;
use openpulse_daemon::{ControlServer, ControlServerConfig, ControlServerHandle};
use openpulse_modem::event::EngineEvent;
use openpulse_modem::ModemEngine;
use tokio::time::timeout;

const MODE: &str = "BPSK250";
/// Larger than the engine's 64-slot ring (so the forwarder must lap) and smaller than the
/// `ControlEvent` ring of 256 (so any loss observed below is the engine ring, not this test's own
/// subscriber).
const FLOOD: usize = 200;
/// A payload length no flood frame uses, so the post-overflow event is identifiable by `bytes`.
const MARKER_PAYLOAD: &[u8] = b"post-overflow marker frame payload......";

fn make_engine() -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::new()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    engine
}

async fn spawn_server(engine: &ModemEngine) -> (SocketAddr, ControlServerHandle) {
    let mut addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
    let handle = ControlServer::spawn(
        "127.0.0.1:0".parse().expect("addr"),
        engine,
        ControlServerConfig {
            initial_mode: MODE.into(),
            initial_station_id: ("N0CALL".into(), "AA00".into()),
            initial_qsy_enabled: false,
            initial_bandplan_mode: "unrestricted".into(),
            initial_allow_tuner_on_high_swr: false,
            control_psk: None,
        },
        Some(&mut addr),
    )
    .await
    .expect("spawn control server");
    (addr, handle)
}

#[tokio::test]
async fn the_event_forwarder_resumes_after_a_ring_overflow() {
    let mut engine = make_engine();
    let (_addr, handle) = spawn_server(&engine).await;
    let mut rx = handle.event_tx.subscribe();

    for _ in 0..FLOOD {
        engine.transmit(b"x", MODE, None).expect("flood transmit");
    }

    // Let the forwarder run and drain everything it produced, to quiescence: two consecutive quiet
    // polls after at least one event, so the marker below is not raced by a straggler.
    let forwarded = timeout(Duration::from_secs(10), async {
        let mut forwarded = 0usize;
        let mut quiet = 0u32;
        while quiet < 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut got = 0usize;
            while rx.try_recv().is_ok() {
                got += 1;
            }
            forwarded += got;
            if got == 0 && forwarded > 0 {
                quiet += 1;
            } else {
                quiet = 0;
            }
        }
        forwarded
    })
    .await
    .expect("forwarder delivered no events at all");
    // Any loss at all proves the lap: this subscriber cannot itself have lost anything (`FLOOD` is
    // under the 256-slot `ControlEvent` ring and it is drained to quiescence), so a shortfall can
    // only be the engine ring lapping. Measured 65 of 200 — the 64-slot ring exactly, plus one
    // `Metrics` event the 1 Hz task puts on the same `ControlEvent` channel before `block_in_place`
    // kills it on this current-thread runtime. The count is every `ControlEvent`, not only engine
    // events, so extra non-engine events only make this assertion stricter.
    assert!(
        forwarded < FLOOD,
        "the engine ring never lapped ({forwarded} of {FLOOD} events forwarded), so this test \
         would pass without exercising the Lagged path at all"
    );

    engine
        .transmit(MARKER_PAYLOAD, MODE, None)
        .expect("marker transmit");

    // Non-engine events (the 1 Hz metrics task shares this channel) are skipped rather than failed
    // on: they say nothing about the forwarder, and failing on one would be a flake against a
    // property that holds.
    let expected_bytes = MARKER_PAYLOAD.len() + 10; // frame envelope
    let bytes = timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await.expect("event channel closed") {
                ControlEvent::EngineEvent {
                    event: EngineEvent::FrameTransmitted { bytes, .. },
                } => return bytes,
                _ => continue,
            }
        }
    })
    .await
    .expect("no engine event after the overflow — the forwarder froze on Lagged");
    assert_eq!(
        bytes, expected_bytes,
        "the post-overflow event is not the marker frame"
    );
}
