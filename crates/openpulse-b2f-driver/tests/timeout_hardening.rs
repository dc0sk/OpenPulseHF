//! A peer must not be able to hold this process open forever by dribbling bytes or going silent.

use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use openpulse_b2f_driver::{CmdPort, DataPort, DriverError};

/// A "TNC" that sends one byte every `gap`, forever — never completing a frame or a line.
fn mock_dribbler(gap: Duration) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        loop {
            if stream.write_all(b"A").is_err() || stream.flush().is_err() {
                return;
            }
            thread::sleep(gap);
        }
    });
    addr
}

/// Run `f` on a worker thread and fail if it hasn't returned within `budget`.
fn must_finish<T: Send + 'static>(
    budget: Duration,
    what: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(budget)
        .unwrap_or_else(|_| panic!("{what} never returned — the read timeout is per-syscall, so a slow drip resets it forever"))
}

/// `SO_RCVTIMEO` restarts on every partial read, so a peer sending one byte per interval keeps a
/// read alive indefinitely even though the connection is useless. The timeout must be a deadline for
/// the whole operation (audit 2026-07-17, medium tier).
#[test]
fn data_port_read_times_out_against_a_slow_drip() {
    let addr = mock_dribbler(Duration::from_millis(50));
    let stream = TcpStream::connect(addr).unwrap();
    let mut data = DataPort::new(stream);
    data.set_timeout(Some(Duration::from_millis(300))).unwrap();

    let err = must_finish(Duration::from_secs(5), "DataPort::recv_frame", move || {
        data.recv_frame()
    });
    assert!(
        matches!(err, Err(DriverError::Timeout)),
        "expected a cumulative-deadline timeout, got {err:?}"
    );
}

/// Same defect on the command port: a newline that never arrives, one byte at a time.
#[test]
fn command_port_read_times_out_against_a_slow_drip() {
    let addr = mock_dribbler(Duration::from_millis(50));
    let stream = TcpStream::connect(addr).unwrap();
    let mut cmd = CmdPort::new(stream).unwrap();
    cmd.set_timeout(Some(Duration::from_millis(300))).unwrap();

    let err = must_finish(Duration::from_secs(5), "CmdPort::read_line", move || {
        cmd.read_line()
    });
    assert!(
        matches!(err, Err(DriverError::Timeout)),
        "expected a cumulative-deadline timeout, got {err:?}"
    );
}

/// A port constructed directly (not via `connect`) must still carry a read timeout — otherwise a
/// silent peer hangs the caller forever with no way to notice.
#[test]
fn ports_have_a_default_read_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let _keep_open = listener.accept();
        thread::sleep(Duration::from_secs(30));
    });

    let stream = TcpStream::connect(addr).unwrap();
    assert!(
        stream.read_timeout().unwrap().is_none(),
        "precondition: a raw TcpStream has no read timeout"
    );
    let cmd = CmdPort::new(stream).unwrap();
    assert!(
        cmd.timeout().is_some(),
        "CmdPort::new must install a default read timeout"
    );

    let stream = TcpStream::connect(addr).unwrap();
    let data = DataPort::new(stream);
    assert!(
        data.timeout().is_some(),
        "DataPort::new must install a default read timeout"
    );
}
