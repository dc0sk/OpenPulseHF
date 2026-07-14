use anyhow::{bail, Context, Result};
use openpulse_radio::{Cm108Ptt, NoOpPtt, PttController, RigctldPtt, VoxPtt};

/// Construct a `PttController` from CLI `--ptt`, `--rig`, and `--rig-file` args.
pub fn build_ptt_controller(
    ptt: &str,
    rig: &str,
    rig_file: &str,
) -> Result<Box<dyn PttController>> {
    match ptt {
        "none" => Ok(Box::new(NoOpPtt::new())),
        "vox" => Ok(Box::new(VoxPtt::new())),
        "rigctld" => {
            let addr = if rig.is_empty() {
                "localhost:4532"
            } else {
                rig
            };
            let ctrl = RigctldPtt::connect(addr).context("failed to connect to rigctld")?;
            Ok(Box::new(ctrl))
        }
        "cm108" => {
            // `--rig` doubles as the device path (empty = auto-detect); GPIO 3 default.
            let ctrl = Cm108Ptt::open(rig, openpulse_radio::cm108::CM108_DEFAULT_GPIO)
                .context("failed to open CM108 HID device for PTT")?;
            Ok(Box::new(ctrl))
        }
        "rts" | "dtr" => {
            #[cfg(feature = "serial")]
            {
                use openpulse_radio::serial::{SerialPin, SerialRtsDtrPtt};
                let pin = if ptt == "rts" {
                    SerialPin::Rts
                } else {
                    SerialPin::Dtr
                };
                if rig.is_empty() {
                    bail!("--rig must specify the serial port path for PTT mode '{ptt}'");
                }
                let ctrl = SerialRtsDtrPtt::open(rig, pin)
                    .context("failed to open serial port for PTT")?;
                Ok(Box::new(ctrl))
            }
            #[cfg(not(feature = "serial"))]
            bail!("serial PTT not compiled in; rebuild with --features serial")
        }
        "generic" => {
            if rig.is_empty() {
                bail!("--rig must specify the serial port path for the generic CAT backend");
            }
            if rig_file.is_empty() {
                bail!("--rig-file must specify the TOML rig-definition file for the generic CAT backend");
            }
            #[cfg(all(unix, feature = "generic-serial"))]
            {
                use openpulse_radio::GenericSerialCat;
                let ctrl =
                    GenericSerialCat::open(rig, rig_file).context("opening generic serial CAT")?;
                Ok(Box::new(ctrl))
            }
            #[cfg(not(all(unix, feature = "generic-serial")))]
            bail!("generic serial CAT not compiled in; rebuild with --features generic-serial (unix only)")
        }
        other => {
            bail!("unknown PTT backend '{other}'; valid values: none | rts | dtr | vox | rigctld | cm108 | generic")
        }
    }
}
