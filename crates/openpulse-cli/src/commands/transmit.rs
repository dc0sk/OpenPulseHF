use anyhow::{bail, Context, Result};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;
use openpulse_radio::SharedPtt;

use crate::commands::bandplan_guard::enforce_mode_guardrails;

pub fn run(
    data: &str,
    mode: &str,
    fec: FecMode,
    device: Option<&str>,
    engine: &mut ModemEngine,
    ptt: &SharedPtt,
) -> Result<()> {
    enforce_mode_guardrails(mode)?;

    // Refuse before keying, rather than be force-released mid-frame (#1299).
    //
    // A single legitimate frame can outlast the PTT watchdog: measured on BPSK31, `Concatenated`
    // and `SoftConcatenated` reach ~265 s at a 223 B payload and `Turbo` ~296 s at 250 B, against
    // a 180 s `DEFAULT_PTT_MAX`. The cliff is the RS block boundary — the same payload at 200 B is
    // 134 s — so this cannot be judged from the mode alone and is computed from the real frame.
    //
    // Starting such a transmission would key the rig, have the watchdog release it ~85 s in, and
    // leave the engine writing the rest of the frame into an unkeyed transmitter: no decode at the
    // far end, and airtime spent for nothing. Refusing costs the operator one error message.
    let airtime = engine
        .tx_airtime_seconds(data.as_bytes(), mode, fec)
        .context("could not determine this frame's airtime")?;
    let deadline = ptt.max_duration().as_secs_f64();
    if airtime > deadline {
        bail!(
            "this frame needs {airtime:.0} s of continuous keying, but the PTT watchdog releases \
             at {deadline:.0} s.\n\
             Nothing was transmitted and the rig was not keyed.\n\
             Try: a lighter FEC (--fec rs), a faster mode, or a shorter payload."
        );
    }

    // The guard releases on drop — including on an early return or an unwind inside `transmit`,
    // which the hand-rolled assert/release pair this replaced did not cover.
    let _guard = ptt.keyed(None).context("PTT assert failed")?;
    engine
        .transmit_with_fec_mode(data.as_bytes(), mode, fec, device)
        .context("transmit failed")?;
    drop(_guard);

    println!(
        "Transmitted {} bytes in {mode} mode (fec={fec:?}).",
        data.len()
    );
    Ok(())
}
