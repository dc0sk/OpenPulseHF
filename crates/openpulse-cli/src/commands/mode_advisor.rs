use anyhow::Result;

use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

fn speed_level_label(level: SpeedLevel) -> &'static str {
    match level {
        SpeedLevel::Sl1 => "SL1",
        SpeedLevel::Sl2 => "SL2",
        SpeedLevel::Sl3 => "SL3",
        SpeedLevel::Sl4 => "SL4",
        SpeedLevel::Sl5 => "SL5",
        SpeedLevel::Sl6 => "SL6",
        SpeedLevel::Sl7 => "SL7",
        SpeedLevel::Sl8 => "SL8",
        SpeedLevel::Sl9 => "SL9",
        SpeedLevel::Sl10 => "SL10",
        SpeedLevel::Sl11 => "SL11",
        SpeedLevel::Sl12 => "SL12",
        SpeedLevel::Sl13 => "SL13",
        SpeedLevel::Sl14 => "SL14",
        SpeedLevel::Sl15 => "SL15",
        SpeedLevel::Sl16 => "SL16",
        SpeedLevel::Sl17 => "SL17",
        SpeedLevel::Sl18 => "SL18",
        SpeedLevel::Sl19 => "SL19",
        SpeedLevel::Sl20 => "SL20",
    }
}

fn recommend_hf_level(snr_db: f32) -> (SpeedLevel, &'static str) {
    if snr_db < 3.0 {
        (
            SpeedLevel::Sl2,
            "Low SNR: prioritize robustness with BPSK31.",
        )
    } else if snr_db < 5.0 {
        (
            SpeedLevel::Sl3,
            "Marginal SNR: step up to BPSK63 with conservative margin.",
        )
    } else if snr_db < 9.0 {
        (
            SpeedLevel::Sl4,
            "Stable low-mid SNR: BPSK250 is appropriate.",
        )
    } else if snr_db < 11.0 {
        (
            SpeedLevel::Sl5,
            "Moderate SNR: QPSK250 is viable with FEC headroom.",
        )
    } else if snr_db < 14.0 {
        (
            SpeedLevel::Sl6,
            "Good SNR: QPSK500 should maintain low FER.",
        )
    } else {
        (
            SpeedLevel::Sl7,
            "High SNR: 8PSK500 recommended for higher throughput.",
        )
    }
}

pub fn run(snr_db: f32) -> Result<()> {
    let profile = SessionProfile::hpx_hf();
    let (level, reason) = recommend_hf_level(snr_db);
    let mode = profile.mode_for(level).unwrap_or("UNMAPPED");

    println!(
        "snr_db={snr_db:.1} recommended_speed_level={} recommended_mode={} reason=\"{}\"",
        speed_level_label(level),
        mode,
        reason
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_expected_levels_for_thresholds() {
        let cases = [
            (-1.0, SpeedLevel::Sl2),
            (2.9, SpeedLevel::Sl2),
            (3.0, SpeedLevel::Sl3),
            (4.9, SpeedLevel::Sl3),
            (5.0, SpeedLevel::Sl4),
            (8.9, SpeedLevel::Sl4),
            (9.0, SpeedLevel::Sl5),
            (10.9, SpeedLevel::Sl5),
            (11.0, SpeedLevel::Sl6),
            (13.9, SpeedLevel::Sl6),
            (14.0, SpeedLevel::Sl7),
        ];

        for (snr, expected_level) in cases {
            let (level, _) = recommend_hf_level(snr);
            assert_eq!(level, expected_level, "snr={snr}");
        }
    }
}
