//! The command port must survive a hostile/broken TNC on the other end.

use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

use openpulse_b2f_driver::{CmdPort, DriverError};

/// A "TNC" that sends `n` bytes with no newline at all, then closes.
fn mock_newline_starved(n: usize) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let chunk = vec![b'A'; 4096];
        let mut sent = 0usize;
        while sent < n {
            let take = chunk.len().min(n - sent);
            if stream.write_all(&chunk[..take]).is_err() {
                return;
            }
            sent += take;
        }
        let _ = stream.flush();
    });
    (addr, handle)
}

/// `read_line` grows its destination without limit, so a peer that never sends a newline can drive
/// the client's memory (the client-side twin of the already-fixed ARDOP server bug: audit 2026-07-17,
/// medium tier). A newline-less blob must be REJECTED, not returned.
#[test]
fn command_port_rejects_a_newline_starved_drip() {
    let (addr, handle) = mock_newline_starved(1024 * 1024);
    let stream = TcpStream::connect(addr).unwrap();
    let mut cmd = CmdPort::new(stream).unwrap();

    match cmd.read_line() {
        Err(DriverError::Ardop(msg)) => {
            assert!(
                msg.contains("too long"),
                "expected an over-length rejection, got: {msg}"
            );
        }
        Ok(line) => panic!(
            "command port buffered {} bytes of newline-less input instead of capping it",
            line.len()
        ),
        Err(e) => panic!("expected an over-length rejection, got {e:?}"),
    }
    let _ = handle.join();
}

/// The cap must not clip ordinary command traffic.
#[test]
fn command_port_still_reads_a_normal_line() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        write!(stream, "CONNECTED PEER\r\nMYID W1AW\r\n").unwrap();
        stream.flush().unwrap();
    });

    let stream = TcpStream::connect(addr).unwrap();
    let mut cmd = CmdPort::new(stream).unwrap();
    assert_eq!(cmd.read_line().unwrap(), "CONNECTED PEER");
    assert_eq!(cmd.read_line().unwrap(), "MYID W1AW");
    let _ = handle.join();
}
