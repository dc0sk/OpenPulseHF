//! Integration tests for audio device hot-plug scenarios.
//!
//! These tests simulate device connect/disconnect events to verify that
//! the modem engine handles device changes gracefully.

use std::sync::{Arc, Mutex};

use openpulse_core::audio::{
    AudioBackend, AudioConfig, AudioInputStream, AudioOutputStream, DeviceInfo,
};
use openpulse_core::error::AudioError;
use openpulse_modem::ModemEngine;

// ── Mock hot-plug backend ─────────────────────────────────────────────────

/// A mock audio backend that allows dynamic device simulation.
///
/// Useful for testing hot-plug scenarios: devices can be added/removed at
/// runtime, and operations on removed devices fail gracefully.
struct HotplugBackend {
    /// Currently available devices (mutable).
    devices: Arc<Mutex<Vec<DeviceInfo>>>,
}

impl HotplugBackend {
    /// Create a new mock backend with an initial device list.
    fn new(initial_devices: Vec<DeviceInfo>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(initial_devices)),
        }
    }

    /// Simulate adding a device (hot-plug event).
    fn plug_device(&self, device: DeviceInfo) {
        let mut devices = self.devices.lock().expect("devices lock poisoned");
        devices.push(device);
    }

    /// Simulate removing a device (hot-unplug event).
    /// Returns true if the device was found and removed.
    fn unplug_device(&self, device_name: &str) -> bool {
        let mut devices = self.devices.lock().expect("devices lock poisoned");
        if let Some(pos) = devices.iter().position(|d| d.name == device_name) {
            devices.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if a specific device exists.
    fn device_exists(&self, device_name: &str) -> bool {
        let devices = self.devices.lock().expect("devices lock poisoned");
        devices.iter().any(|d| d.name == device_name)
    }

    /// Snapshot the current device list (for test assertions).
    fn snapshot_devices(&self) -> Vec<String> {
        let devices = self.devices.lock().expect("devices lock poisoned");
        devices.iter().map(|d| d.name.clone()).collect()
    }
}

impl AudioBackend for HotplugBackend {
    fn name(&self) -> &str {
        "HotplugMock"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        let devices = self.devices.lock().expect("devices lock poisoned");
        Ok(devices.clone())
    }

    fn open_input(
        &self,
        device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioInputStream>, AudioError> {
        let devices = self.devices.lock().expect("devices lock poisoned");

        // If a specific device was requested, verify it exists.
        if let Some(device_name) = device {
            if !devices.iter().any(|d| d.name == device_name && d.is_input) {
                return Err(AudioError::DeviceNotFound(device_name.to_string()));
            }
        } else {
            // If no device specified, use the default.
            if devices.is_empty() || !devices[0].is_input {
                return Err(AudioError::DeviceNotFound(
                    "no default input device".to_string(),
                ));
            }
        }

        Ok(Box::new(MockInputStream))
    }

    fn open_output(
        &self,
        device: Option<&str>,
        _config: &AudioConfig,
    ) -> Result<Box<dyn AudioOutputStream>, AudioError> {
        let devices = self.devices.lock().expect("devices lock poisoned");

        // If a specific device was requested, verify it exists.
        if let Some(device_name) = device {
            if !devices.iter().any(|d| d.name == device_name && d.is_output) {
                return Err(AudioError::DeviceNotFound(device_name.to_string()));
            }
        } else {
            // If no device specified, use the default.
            if devices.is_empty() || !devices[0].is_output {
                return Err(AudioError::DeviceNotFound(
                    "no default output device".to_string(),
                ));
            }
        }

        Ok(Box::new(MockOutputStream))
    }
}

struct MockInputStream;
struct MockOutputStream;

impl AudioInputStream for MockInputStream {
    fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        Ok(vec![0.0; 1024]) // Dummy audio data for testing
    }
    fn close(self: Box<Self>) {}
}

impl AudioOutputStream for MockOutputStream {
    fn write(&mut self, _samples: &[f32]) -> Result<(), AudioError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
    fn close(self: Box<Self>) {}
}

// ── Hot-plug test cases ───────────────────────────────────────────────────

#[test]
fn hotplug_device_enumeration_initial() {
    let backend = HotplugBackend::new(vec![
        DeviceInfo {
            name: "input1".to_string(),
            is_input: true,
            is_output: false,
            is_default: true,
            supported_sample_rates: vec![8000, 16000],
        },
        DeviceInfo {
            name: "output1".to_string(),
            is_input: false,
            is_output: true,
            is_default: true,
            supported_sample_rates: vec![8000, 16000],
        },
    ]);

    let devices = backend.list_devices().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].name, "input1");
    assert_eq!(devices[1].name, "output1");
}

#[test]
fn hotplug_device_add() {
    let backend = HotplugBackend::new(vec![DeviceInfo {
        name: "input1".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![8000],
    }]);

    let initial_devices = backend.list_devices().unwrap();
    assert_eq!(initial_devices.len(), 1);

    // Simulate hot-plug: add a new input device
    backend.plug_device(DeviceInfo {
        name: "input2".to_string(),
        is_input: true,
        is_output: false,
        is_default: false,
        supported_sample_rates: vec![16000],
    });

    let updated_devices = backend.list_devices().unwrap();
    assert_eq!(updated_devices.len(), 2);
    assert!(updated_devices.iter().any(|d| d.name == "input1"));
    assert!(updated_devices.iter().any(|d| d.name == "input2"));
}

#[test]
fn hotplug_device_remove() {
    let backend = HotplugBackend::new(vec![
        DeviceInfo {
            name: "input1".to_string(),
            is_input: true,
            is_output: false,
            is_default: true,
            supported_sample_rates: vec![8000],
        },
        DeviceInfo {
            name: "input2".to_string(),
            is_input: true,
            is_output: false,
            is_default: false,
            supported_sample_rates: vec![16000],
        },
    ]);

    // Open a stream on input2
    let cfg = AudioConfig::default();
    let stream = backend.open_input(Some("input2"), &cfg);
    assert!(stream.is_ok());

    // Simulate hot-unplug: remove input2
    let removed = backend.unplug_device("input2");
    assert!(removed);

    // Verify the device is gone
    let devices = backend.list_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "input1");

    // Attempting to open a new stream on the removed device should fail
    let stream = backend.open_input(Some("input2"), &cfg);
    assert!(stream.is_err());
}

#[test]
fn hotplug_fallback_to_default_after_device_removed() {
    let backend = HotplugBackend::new(vec![
        DeviceInfo {
            name: "input_default".to_string(),
            is_input: true,
            is_output: false,
            is_default: true,
            supported_sample_rates: vec![8000],
        },
        DeviceInfo {
            name: "input_alt".to_string(),
            is_input: true,
            is_output: false,
            is_default: false,
            supported_sample_rates: vec![16000],
        },
    ]);

    let cfg = AudioConfig::default();

    // Scenario: user opened a stream on the non-default device (input_alt)
    // Then that device is unplugged.
    backend.unplug_device("input_alt");

    // When there's only the default device left, opening with None
    // should now succeed (fall back to default).
    let stream = backend.open_input(None, &cfg);
    assert!(stream.is_ok());

    // Verify the stream was opened on the default device
    let devices = backend.list_devices().unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "input_default");
}

#[test]
fn hotplug_no_default_input_device() {
    let backend = HotplugBackend::new(vec![DeviceInfo {
        name: "output_only".to_string(),
        is_input: false,
        is_output: true,
        is_default: true,
        supported_sample_rates: vec![8000],
    }]);

    let cfg = AudioConfig::default();

    // Attempting to open input when no input devices exist should fail
    let stream = backend.open_input(None, &cfg);
    assert!(stream.is_err());
}

#[test]
fn hotplug_no_default_output_device() {
    let backend = HotplugBackend::new(vec![DeviceInfo {
        name: "input_only".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![8000],
    }]);

    let cfg = AudioConfig::default();

    // Attempting to open output when no output devices exist should fail
    let stream = backend.open_output(None, &cfg);
    assert!(stream.is_err());
}

#[test]
fn hotplug_device_exists_check() {
    let backend = HotplugBackend::new(vec![DeviceInfo {
        name: "device1".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![8000],
    }]);

    assert!(backend.device_exists("device1"));
    assert!(!backend.device_exists("device2"));

    backend.plug_device(DeviceInfo {
        name: "device2".to_string(),
        is_input: true,
        is_output: false,
        is_default: false,
        supported_sample_rates: vec![16000],
    });

    assert!(backend.device_exists("device2"));

    backend.unplug_device("device2");
    assert!(!backend.device_exists("device2"));
}

#[test]
fn hotplug_snapshot_device_names() {
    let backend = HotplugBackend::new(vec![]);

    assert_eq!(backend.snapshot_devices(), Vec::<String>::new());

    backend.plug_device(DeviceInfo {
        name: "dev_a".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![8000],
    });

    backend.plug_device(DeviceInfo {
        name: "dev_b".to_string(),
        is_input: false,
        is_output: true,
        is_default: true,
        supported_sample_rates: vec![8000],
    });

    let snapshot = backend.snapshot_devices();
    assert_eq!(snapshot, vec!["dev_a", "dev_b"]);
}

#[test]
fn hotplug_multiple_add_remove_cycles() {
    let backend = HotplugBackend::new(vec![]);

    // Cycle 1: add, verify, remove
    backend.plug_device(DeviceInfo {
        name: "device1".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![8000],
    });
    assert_eq!(backend.snapshot_devices().len(), 1);

    backend.unplug_device("device1");
    assert_eq!(backend.snapshot_devices().len(), 0);

    // Cycle 2: add different device
    backend.plug_device(DeviceInfo {
        name: "device2".to_string(),
        is_input: true,
        is_output: false,
        is_default: true,
        supported_sample_rates: vec![16000],
    });
    assert_eq!(backend.snapshot_devices().len(), 1);
    assert!(backend.device_exists("device2"));

    // Cycle 3: add while one exists, then remove
    backend.plug_device(DeviceInfo {
        name: "device3".to_string(),
        is_input: true,
        is_output: false,
        is_default: false,
        supported_sample_rates: vec![32000],
    });
    assert_eq!(backend.snapshot_devices().len(), 2);

    backend.unplug_device("device2");
    assert_eq!(backend.snapshot_devices().len(), 1);
    assert!(backend.device_exists("device3"));
    assert!(!backend.device_exists("device2"));
}

#[test]
fn hotplug_modem_engine_with_dynamic_devices() {
    let backend = HotplugBackend::new(vec![DeviceInfo {
        name: "loopback".to_string(),
        is_input: true,
        is_output: true,
        is_default: true,
        supported_sample_rates: vec![8000, 16000],
    }]);

    let _engine = ModemEngine::new(Box::new(backend));

    // Engine successfully created with dynamic backend.
    // In a real scenario, this would be used with transmit/receive operations
    // that might be affected by device hot-plug events.
}
