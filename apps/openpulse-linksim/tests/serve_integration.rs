//! Integration tests for the daemon-protocol serve mode.
//!
//! Both the owned-sim path (`serve_on`) and the external-hub path (`serve_hub_on`) start a
//! server on an ephemeral port, connect a raw TCP client (standing in for `openpulse-panel`),
//! and assert it receives both a parseable `ControlEvent` JSON line and a binary `OPSP`
//! spectrum frame.

#![cfg(feature = "serve")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::FecMode;
use openpulse_linksim::serve::{serve_hub_on, serve_on, FrameHub};
use openpulse_linksim::{ChannelSpec, LinkParams, LinkSim};

fn demo_params() -> LinkParams {
    // hpx_wideband starts at a fast mode (QPSK500), so frames are short and events arrive
    // promptly. usize::MAX runs continuously until the client disconnects.
    LinkParams {
        profile_name: "hpx_wideband".into(),
        forward: ChannelSpec::Awgn(20.0),
        reverse: ChannelSpec::Awgn(25.0),
        payload_bytes_per_frame: 32,
        total_frames: usize::MAX,
        fec: FecMode::Rs,
        compression: CompressionAlgorithm::None,
        turnaround_s: 0.2,
        max_attempts: 4,
        seed: 99,
    }
}

/// Read from `stream` until both a JSON `ControlEvent` and an `OPSP` spectrum frame are seen
/// (or the deadline passes). Asserts every JSON line parses and carries a `type` tag.
fn assert_receives_events_and_spectrum(mut stream: TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(b"{\"cmd\":\"subscribe_spectrum\",\"fps\":20}\n")
        .unwrap();

    let mut buf: Vec<u8> = Vec::new();
    let mut saw_json = false;
    let mut saw_spectrum = false;
    let deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < deadline && !(saw_json && saw_spectrum) {
        let mut tmp = [0u8; 8192];
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }

        // Drain complete frames: 'O' => binary OPSP, otherwise a newline-terminated JSON line.
        loop {
            if buf.is_empty() {
                break;
            }
            if buf[0] == b'O' {
                if buf.len() < 10 {
                    break;
                }
                assert_eq!(&buf[0..4], b"OPSP", "spectrum magic");
                let fft = u16::from_le_bytes([buf[4], buf[5]]) as usize;
                let need = 10 + fft * 4;
                if buf.len() < need {
                    break;
                }
                saw_spectrum = true;
                buf.drain(..need);
            } else {
                let Some(nl) = buf.iter().position(|&b| b == b'\n') else {
                    break;
                };
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let text = String::from_utf8_lossy(&line);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(text).expect("each JSON line must parse");
                assert!(v.get("type").is_some(), "ControlEvent has a type tag");
                saw_json = true;
            }
        }
    }

    assert!(saw_json, "client should receive at least one ControlEvent");
    assert!(
        saw_spectrum,
        "client should receive at least one spectrum frame"
    );
}

#[test]
fn owned_sim_client_receives_events_and_spectrum() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let params = demo_params();
    std::thread::spawn(move || {
        let _ = serve_on(listener, &params, 50);
    });

    let stream = TcpStream::connect(addr).expect("connect");
    assert_receives_events_and_spectrum(stream);
}

#[test]
fn hub_client_receives_published_frames() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let hub = FrameHub::new();

    let server_hub = hub.clone();
    std::thread::spawn(move || {
        let _ = serve_hub_on(listener, server_hub);
    });

    // Drive a LinkSim and publish each frame to the hub — the external-producer pattern the
    // GUI uses. Runs until the test process exits.
    let pub_hub = hub.clone();
    std::thread::spawn(move || {
        let mut sim = LinkSim::new(&demo_params());
        while let Some(fs) = sim.step() {
            pub_hub.publish(&fs);
            std::thread::sleep(Duration::from_millis(20));
        }
    });

    let stream = TcpStream::connect(addr).expect("connect");
    assert_receives_events_and_spectrum(stream);
}
