//! Periodic beacon transmission scheduler.

use crate::beacon::AuthBeacon;
use crate::data_port::{DataPortError, FreeDvDataPort};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Sends a signed [`AuthBeacon`] at a fixed interval via [`FreeDvDataPort`].
pub struct BeaconScheduler {
    interval: Duration,
    port: Arc<FreeDvDataPort>,
    /// Factory closure that produces the next beacon to transmit.
    make_beacon: Box<dyn Fn() -> AuthBeacon + Send + Sync>,
}

impl BeaconScheduler {
    pub fn new(
        interval: Duration,
        port: Arc<FreeDvDataPort>,
        make_beacon: impl Fn() -> AuthBeacon + Send + Sync + 'static,
    ) -> Self {
        Self {
            interval,
            port,
            make_beacon: Box::new(make_beacon),
        }
    }

    /// Run the scheduler, firing immediately and then every `interval`.
    ///
    /// Runs until the task is cancelled.
    pub async fn run(&self) {
        loop {
            let beacon = (self.make_beacon)();
            let wire = beacon.encode();
            match self.port.send(&wire).await {
                Ok(()) => info!(callsign = %beacon.callsign, "auth beacon sent"),
                Err(DataPortError::Io(e)) => warn!("beacon send failed: {e}"),
            }
            tokio::time::sleep(self.interval).await;
        }
    }
}
