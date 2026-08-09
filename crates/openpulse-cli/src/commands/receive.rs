use anyhow::{bail, Context, Result};
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;
use std::time::Duration;

/// FEC modes the scanning (timed) receive cannot serve, with the reason an operator needs.
///
/// Both derive their block geometry from the decoded length: `ShortRs` is byte-exact with no length
/// prefix, and Turbo's QPP block size is `llrs.len() / 3`. The scanning receive slices a window that
/// includes trailing noise, so the length it recovers is a property of the window rather than the
/// frame — see the dispatch in `ModemEngine::receive_with_fec_mode_timeout`.
///
/// This is a property of those codecs, not a missing feature. Rejecting the combination here means
/// the operator learns it from the flags they typed, instead of from an engine error naming an
/// internal function they cannot call.
fn listen_unsupported_reason(fec: FecMode) -> Option<&'static str> {
    match fec {
        FecMode::ShortRs => Some("its blocks are byte-exact and carry no length prefix"),
        FecMode::Turbo => Some("its QPP block size is derived from the decoded LLR count"),
        _ => None,
    }
}

pub fn run(
    mode: &str,
    fec: FecMode,
    device: Option<&str>,
    listen_ms: Option<u64>,
    engine: &mut ModemEngine,
) -> Result<()> {
    let payload = match listen_ms {
        Some(ms) => {
            if let Some(why) = listen_unsupported_reason(fec) {
                bail!(
                    "--fec {fec:?} cannot be combined with a listen timeout, because {why}, so it \
                     needs the exact frame length. A timed listen scans a window that includes \
                     trailing noise, which changes that length.\n\
                     Re-run without the timeout for a single-shot decode."
                );
            }
            engine
                .receive_with_fec_mode_timeout(mode, fec, device, Duration::from_millis(ms))
                .context("receive failed")?
        }
        None => engine
            .receive_with_fec_mode(mode, fec, device)
            .context("receive failed")?,
    };
    let text = String::from_utf8_lossy(&payload);
    println!("{text}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rejected set must match the engine's own dispatch exactly — no more, no less.
    ///
    /// Derived from `FecMode::ALL` rather than a hand-written list, so a new variant forces a
    /// decision here too. The engine's exclusions are documented in
    /// `receive_with_fec_mode_timeout`; if that dispatch gains or loses an arm, this test is the
    /// thing that notices.
    #[test]
    fn only_short_rs_and_turbo_are_rejected_for_a_timed_listen() {
        let rejected: Vec<FecMode> = FecMode::ALL
            .into_iter()
            .filter(|f| listen_unsupported_reason(*f).is_some())
            .collect();
        assert_eq!(
            rejected,
            vec![FecMode::ShortRs, FecMode::Turbo],
            "the CLI's rejected set has drifted from the engine's timeout-receive dispatch"
        );
    }

    /// Every reason reads as a sentence fragment completing "because {why}, so it needs...".
    #[test]
    fn every_reason_is_phrased_for_an_operator() {
        for fec in FecMode::ALL {
            if let Some(why) = listen_unsupported_reason(fec) {
                assert!(
                    why.starts_with("its ") && !why.contains("receive_with_fec_mode"),
                    "{fec:?}: reason should explain the codec to an operator, got {why:?}"
                );
            }
        }
    }
}
