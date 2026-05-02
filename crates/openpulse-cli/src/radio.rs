use anyhow::{bail, Context, Result};
use openpulse_radio::{NoOpPtt, PttController, RigctldPtt, VoxPtt};

/// Construct a `PttController` from CLI `--ptt` and `--rig` args.
pub fn build_ptt_controller(ptt: &str, rig: &str) -> Result<Box<dyn PttController>> {
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
        other => {
            bail!("unknown PTT backend '{other}'; valid values: none | rts | dtr | vox | rigctld")
        }
    }
}
