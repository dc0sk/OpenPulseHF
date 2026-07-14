//! CM108/CM109/CM119 sound-chip GPIO PTT over USB-HID (REQ-PTT-02).
//!
//! The cheap-USB-interface PTT path (DMK URI, RA-series, AIOC, homebrew). Keying is a single 5-byte HID
//! output report written to `/dev/hidrawN` — a plain `std::fs::File` write, so this backend needs **no**
//! `hidapi`/C dependency (keeps the `cross`-compile gate clean) and is always compiled. The Linux sysfs
//! auto-detect is the only target-gated part.
//!
//! Report layout (canonical, per Dire Wolf `cm108.c`): `io[0]=0` (report id), `io[1]=0`, `io[2]=iodata`
//! (GPIO output values), `io[3]=iomask` (GPIO direction, 1=output), `io[4]=0`. GPIO pin `n` (1..=8) maps
//! to bit `1 << (n-1)`; **GPIO 3** is the de-facto PTT pin on essentially all products and homebrew.

use crate::{PttController, PttError};

/// C-Media USB vendor id (all CM108/CM109/CM119 variants).
const CM108_VENDOR_ID: u32 = 0x0d8c;

/// The default PTT GPIO pin — GPIO 3, used by essentially every CM108 interface.
pub const CM108_DEFAULT_GPIO: u8 = 3;

/// Encode the 5-byte CM108 HID output report that drives `gpio` (1..=8) to `asserted`.
///
/// `iomask` is always set for the pin (declares it an output); `iodata` carries the level. Pure and
/// hardware-free — this is the unit-tested keying artifact.
pub fn cm108_ptt_report(gpio: u8, asserted: bool) -> [u8; 5] {
    let bit = 1u8 << (gpio.saturating_sub(1) & 0x07);
    let iodata = if asserted { bit } else { 0 };
    let iomask = bit;
    [0x00, 0x00, iodata, iomask, 0x00]
}

/// Whether a `/sys/class/hidraw/*/device/uevent` body is a CM108-family device (vendor `0x0d8c`).
///
/// The `HID_ID=bus:vendor:product` line carries the vendor as 8 hex digits (e.g. `0003:00000D8C:...`).
fn uevent_is_cm108(uevent: &str) -> bool {
    uevent.lines().any(|line| {
        line.strip_prefix("HID_ID=")
            .and_then(|v| v.split(':').nth(1))
            .and_then(|vendor| u32::from_str_radix(vendor.trim(), 16).ok())
            .map(|vendor| vendor == CM108_VENDOR_ID)
            .unwrap_or(false)
    })
}

/// Scan `/sys/class/hidraw` for the first CM108-family device, returning its `/dev/hidrawN` path.
#[cfg(target_os = "linux")]
fn detect_cm108_hidraw() -> Option<String> {
    let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort(); // deterministic: lowest hidrawN wins
    for name in names {
        let uevent_path = format!("/sys/class/hidraw/{name}/device/uevent");
        if let Ok(body) = std::fs::read_to_string(&uevent_path) {
            if uevent_is_cm108(&body) {
                return Some(format!("/dev/{name}"));
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn detect_cm108_hidraw() -> Option<String> {
    None
}

/// CM108 GPIO PTT controller over a `/dev/hidrawN` HID device.
pub struct Cm108Ptt {
    file: std::fs::File,
    gpio: u8,
    asserted: bool,
}

impl Cm108Ptt {
    /// Open a CM108 HID device and key PTT on `gpio` (1..=8). `device` is a `/dev/hidrawN` path, or empty
    /// to auto-detect the first CM108-family device (Linux). Leaves PTT released.
    pub fn open(device: &str, gpio: u8) -> Result<Self, PttError> {
        if !(1..=8).contains(&gpio) {
            return Err(PttError::Serial(format!(
                "CM108 GPIO pin {gpio} out of range 1..=8"
            )));
        }
        let path = if device.is_empty() {
            detect_cm108_hidraw().ok_or_else(|| {
                PttError::Serial(
                    "no CM108-family HID device found (set [modem] ptt_device to a /dev/hidrawN path)"
                        .into(),
                )
            })?
        } else {
            device.to_string()
        };
        let file = std::fs::OpenOptions::new().write(true).open(&path)?;
        let mut ctrl = Self {
            file,
            gpio,
            asserted: false,
        };
        ctrl.write_report(false)?; // declare the pin an output and ensure released
        Ok(ctrl)
    }

    fn write_report(&mut self, asserted: bool) -> Result<(), PttError> {
        use std::io::Write;
        self.file
            .write_all(&cm108_ptt_report(self.gpio, asserted))?;
        Ok(())
    }
}

impl PttController for Cm108Ptt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.write_report(true)?;
        self.asserted = true;
        Ok(())
    }

    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.write_report(false)?;
        self.asserted = false;
        Ok(())
    }

    fn is_asserted(&self) -> bool {
        self.asserted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_encodes_gpio3_assert_and_release() {
        // GPIO 3 → bit 0x04. Assert: iodata=iomask=0x04; release: iodata=0, iomask=0x04.
        assert_eq!(cm108_ptt_report(3, true), [0x00, 0x00, 0x04, 0x04, 0x00]);
        assert_eq!(cm108_ptt_report(3, false), [0x00, 0x00, 0x00, 0x04, 0x00]);
    }

    #[test]
    fn report_maps_each_pin_to_its_bit() {
        for gpio in 1..=8u8 {
            let bit = 1u8 << (gpio - 1);
            assert_eq!(cm108_ptt_report(gpio, true), [0, 0, bit, bit, 0]);
            assert_eq!(cm108_ptt_report(gpio, false), [0, 0, 0, bit, 0]);
        }
    }

    #[test]
    fn uevent_matches_cmedia_vendor() {
        let cm108 =
            "DRIVER=hid-generic\nHID_ID=0003:00000D8C:0000000C\nHID_NAME=USB Audio Device\n";
        assert!(uevent_is_cm108(cm108));
        // lowercase vendor also matches
        assert!(uevent_is_cm108("HID_ID=0003:00000d8c:00000013\n"));
    }

    #[test]
    fn uevent_rejects_other_and_malformed() {
        assert!(!uevent_is_cm108(
            "HID_ID=0003:0000046D:0000C52B\nHID_NAME=Some Keyboard\n"
        )); // Logitech
        assert!(!uevent_is_cm108("HID_NAME=no id line\n"));
        assert!(!uevent_is_cm108("HID_ID=garbage\n"));
        assert!(!uevent_is_cm108(""));
    }

    #[test]
    fn open_rejects_out_of_range_gpio() {
        assert!(Cm108Ptt::open("/dev/null", 0).is_err());
        assert!(Cm108Ptt::open("/dev/null", 9).is_err());
    }

    #[test]
    fn open_errors_on_a_missing_device() {
        let r = Cm108Ptt::open("/dev/nonexistent-openpulse-hidraw-xyz", 3);
        assert!(
            r.is_err(),
            "opening a missing HID device must error, not panic"
        );
    }
}
