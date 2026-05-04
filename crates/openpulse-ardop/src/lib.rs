//! ARDOP-compatible TNC server.
//!
//! Exposes two TCP ports that Winlink and other ARQ applications can connect to:
//!
//! - **Command port** (default 8515): ASCII line protocol for TNC control.
//! - **Data port** (default 8516): binary `u16 BE` length-prefixed framing for
//!   payload data in both directions.

mod bridge;
mod command;
mod data;
pub mod error;
mod state;

pub use bridge::{spawn_worker, ModemBridge};
pub use error::ArdopError;
pub use state::TncState;

use std::sync::Arc;

use tokio::net::TcpListener;

use openpulse_modem::ModemEngine;

/// TNC server configuration.
#[derive(Debug, Clone)]
pub struct ArdopConfig {
    /// Bind address for the command port (default `127.0.0.1`).
    pub bind_addr: String,
    /// Command port number (ARDOP default: 8515).
    pub command_port: u16,
    /// Data port number (ARDOP default: 8516).
    pub data_port: u16,
    /// Default modem mode used for transmit/receive operations.
    pub mode: String,
    /// When `true`, TX data is echoed back as RX data without going through
    /// the modem engine.  Useful for protocol-level integration tests.
    pub loopback: bool,
}

impl Default for ArdopConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1".into(),
            command_port: 8515,
            data_port: 8516,
            mode: "BPSK250".into(),
            loopback: false,
        }
    }
}

/// ARDOP-compatible TNC server.
pub struct ArdopServer {
    bridge: Arc<ModemBridge>,
    config: ArdopConfig,
}

impl ArdopServer {
    pub fn new(engine: ModemEngine, config: ArdopConfig) -> Self {
        let (bridge, tx_data_rx) = ModemBridge::new(engine, config.mode.clone(), config.loopback);
        spawn_worker(bridge.clone(), tx_data_rx);
        Self { bridge, config }
    }

    /// Returns a handle to the shared bridge (useful for testing).
    pub fn bridge(&self) -> Arc<ModemBridge> {
        self.bridge.clone()
    }

    /// Run the command and data port listeners until one of them stops.
    pub async fn run(self) -> Result<(), ArdopError> {
        let addr = &self.config.bind_addr;
        let cmd_listener =
            TcpListener::bind(format!("{addr}:{}", self.config.command_port)).await?;
        let data_listener = TcpListener::bind(format!("{addr}:{}", self.config.data_port)).await?;
        self.run_with_listeners(cmd_listener, data_listener).await
    }

    /// Run using pre-bound listeners (useful for test harnesses that need to
    /// discover the port after binding with port 0).
    pub async fn run_with_listeners(
        self,
        cmd_listener: TcpListener,
        data_listener: TcpListener,
    ) -> Result<(), ArdopError> {
        let cmd_bridge = self.bridge.clone();
        let data_bridge = self.bridge;

        let cmd_handle = tokio::spawn(command::serve(cmd_listener, cmd_bridge));
        let data_handle = tokio::spawn(data::serve(data_listener, data_bridge));

        tokio::select! {
            r = cmd_handle => r.map_err(|_| ArdopError::Join)?,
            r = data_handle => r.map_err(|_| ArdopError::Join)?,
        }
    }
}
