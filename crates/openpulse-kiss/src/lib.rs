//! KISS-over-TCP TNC server for APRS and AX.25 applications.
//!
//! Exposes a single TCP port (default 8100) that APRS clients can connect to.
//! Uses KISS byte-stuffed framing with AX.25 UI frames as the payload.

mod bridge;
mod error;
mod server;

pub mod ax25;
pub mod kiss;

pub use bridge::{spawn_worker, KissBridge, KissConfig};
pub use error::KissTncError;

use std::sync::Arc;

use tokio::net::TcpListener;

use openpulse_modem::ModemEngine;

/// KISS TNC server.
pub struct KissServer {
    bridge: Arc<KissBridge>,
    config: KissConfig,
}

impl KissServer {
    pub fn new(engine: ModemEngine, config: KissConfig) -> Self {
        let (bridge, tx_data_rx) = KissBridge::new(engine, config.mode.clone(), config.loopback);
        spawn_worker(bridge.clone(), tx_data_rx);
        Self { bridge, config }
    }

    /// Returns a handle to the shared bridge (useful for testing).
    pub fn bridge(&self) -> Arc<KissBridge> {
        self.bridge.clone()
    }

    /// Run the KISS TCP listener until it stops.
    pub async fn run(self) -> Result<(), KissTncError> {
        let addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        self.run_with_listener(listener).await
    }

    /// Run using a pre-bound listener (useful for tests that need port 0).
    pub async fn run_with_listener(self, listener: TcpListener) -> Result<(), KissTncError> {
        server::serve(listener, self.bridge).await
    }
}
