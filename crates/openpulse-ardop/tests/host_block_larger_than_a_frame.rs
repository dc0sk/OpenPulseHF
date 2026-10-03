//! A host data block larger than one modem frame goes on the air as frame-sized chunks (#1385).
//!
//! The data port accepted blocks up to 4 096 bytes and handed each whole to the modem, where
//! `Frame::new` refused anything over 255: the transmitter keyed once, nothing was modulated, and
//! the host saw no error. Driven through the real data port of a non-loopback TNC; a spy PTT counts
//! the keyings, one per transmitted frame on the non-adaptive data path.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use openpulse_ardop::{ArdopConfig, ArdopServer};
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::frame::Frame;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};

struct SpyPtt(Arc<AtomicUsize>);
impl PttController for SpyPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        false
    }
}

/// Send one `len`-byte block to a fresh TNC's data port; return the keyings it caused.
async fn keyings_for_block(len: usize) -> usize {
    let cmd = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let data = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (cmd_port, data_port) = (
        cmd.local_addr().unwrap().port(),
        data.local_addr().unwrap().port(),
    );
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    let keyed = Arc::new(AtomicUsize::new(0));
    let server = ArdopServer::with_trust_relay_ptt(
        engine,
        ArdopConfig {
            bind_addr: "127.0.0.1".into(),
            command_port: cmd_port,
            data_port,
            mode: "BPSK250".into(),
            loopback: false,
            auto_id_interval_secs: 0,
            ..Default::default()
        },
        Default::default(),
        None,
        Box::new(SpyPtt(keyed.clone())),
    );
    tokio::spawn(async move {
        let _ = server.run_with_listeners(cmd, data).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let c = TcpStream::connect(("127.0.0.1", cmd_port)).await.unwrap();
    let mut c = BufReader::new(c);
    c.get_mut().write_all(b"MYID DC0SK\r\n").await.unwrap();
    let mut reply = String::new();
    tokio::time::timeout(Duration::from_secs(2), c.read_line(&mut reply))
        .await
        .expect("MYID reply")
        .unwrap();

    let mut s = TcpStream::connect(("127.0.0.1", data_port)).await.unwrap();
    let block: Vec<u8> = (0..len as u32).map(|i| (i * 7 % 251) as u8).collect();
    s.write_all(&(len as u16).to_be_bytes()).await.unwrap();
    s.write_all(&block).await.unwrap();
    s.flush().await.unwrap();

    // Let the worker drain the queue: count until it stops moving.
    let mut last = usize::MAX;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let now = keyed.load(Ordering::SeqCst);
        if now == last && now > 0 {
            break;
        }
        last = now;
    }
    keyed.load(Ordering::SeqCst)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_600_byte_block_goes_out_as_three_frames() {
    let keyings = keyings_for_block(600).await;
    assert_eq!(
        keyings,
        600usize.div_ceil(Frame::MAX_PAYLOAD),
        "a 600-byte host block must key once per frame-sized chunk"
    );
}

/// Control: a block within one frame keys once, as it always did.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_block_within_one_frame_keys_once() {
    assert_eq!(keyings_for_block(200).await, 1);
}
