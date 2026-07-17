use anyhow::Result;

use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;

pub(crate) fn speed_level_label(level: SpeedLevel) -> &'static str {
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

fn recommend_hf_level(
    profile: &SessionProfile,
    profile_name: &str,
    snr_db: f32,
) -> (SpeedLevel, String) {
    let levels = profile.defined_levels();
    // Floor at the most robust rung the profile actually defines (e.g. SL5 for
    // hpx_ofdm_hf), not a hard-coded SL2 that some profiles never map.
    let mut selected = levels.first().copied().unwrap_or(SpeedLevel::Sl2);

    for &level in &levels {
        let Some(floor_db) = profile.snr_floor_for_level(level) else {
            continue;
        };
        if snr_db >= floor_db {
            selected = level;
        }
    }

    let reason = if let Some(floor_db) = profile.snr_floor_for_level(selected) {
        format!(
            "Using profile '{profile_name}' floor: snr_db={snr_db:.1} meets {} floor ({floor_db:.1} dB).",
            speed_level_label(selected)
        )
    } else {
        format!(
            "Using profile '{profile_name}' defaults: snr_db={snr_db:.1} mapped to {}.",
            speed_level_label(selected)
        )
    };

    (selected, reason)
}

/// Recommend a speed level and mode for `snr_db` using the selected session profile.
///
/// Profile resolution order: explicit `--profile` flag > config `[modem] profile` >
/// built-in default (`hpx_hf`).
pub fn run(snr_db: f32, profile_override: Option<&str>) -> Result<()> {
    let name = resolve_profile_name(profile_override);
    let profile = SessionProfile::by_name(&name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown session profile {name:?}; valid profiles: {}",
            SessionProfile::PROFILE_NAMES.join(", ")
        )
    })?;
    let (level, reason) = recommend_hf_level(&profile, &name, snr_db);
    let mode = profile.mode_for(level).unwrap_or("UNMAPPED");

    println!(
        "profile={name} snr_db={snr_db:.1} recommended_speed_level={} recommended_mode={} reason=\"{}\"",
        speed_level_label(level),
        mode,
        reason
    );

    Ok(())
}

/// Profile name from the CLI override, else config `[modem] profile`, else the default.
fn resolve_profile_name(profile_override: Option<&str>) -> String {
    if let Some(name) = profile_override {
        return name.to_string();
    }
    openpulse_config::load()
        .map(|cfg| cfg.modem.profile)
        .unwrap_or_else(|_| "hpx_hf".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_expected_levels_for_thresholds() {
        let profile = SessionProfile::hpx_hf();
        // Thresholds track the fade-aware hpx_hf SNR floors: SL2=3 SL3=4 SL4=4.5 SL5=5 SL6=7
        // SL7=9 SL8=10 SL9=12 SL10=14 SL11=16 SL12=18 SL13=19 SL14=20 (ladder top). The OFDM rungs'
        // floors are plugin symbol-domain SNR, not AWGN channel SNR — see profile.rs.
        // recommend_hf_level picks the highest rung whose floor ≤ snr. Keep in sync if floors change.
        let cases = [
            // Below BPSK31's 3 dB floor → the MFSK16 non-coherent sub-floor rung (SL1), the lowest
            // defined level (REQ-WSIG-01). At/above 3 dB, BPSK31 (SL2) and up.
            (-1.0, SpeedLevel::Sl1),
            (2.9, SpeedLevel::Sl1),
            (3.0, SpeedLevel::Sl2),
            (3.9, SpeedLevel::Sl2),
            (4.0, SpeedLevel::Sl3),
            (4.5, SpeedLevel::Sl4),
            (4.9, SpeedLevel::Sl4),
            (5.0, SpeedLevel::Sl5),
            (6.9, SpeedLevel::Sl5),
            (7.0, SpeedLevel::Sl6),
            (8.9, SpeedLevel::Sl6),
            (9.0, SpeedLevel::Sl7),
            (9.9, SpeedLevel::Sl7),
            (10.0, SpeedLevel::Sl8),
            (11.9, SpeedLevel::Sl8),
            (12.0, SpeedLevel::Sl9),
            (13.9, SpeedLevel::Sl9),
            (14.0, SpeedLevel::Sl10),
            (15.9, SpeedLevel::Sl10),
            (16.0, SpeedLevel::Sl11),
            (17.9, SpeedLevel::Sl11),
            (18.0, SpeedLevel::Sl12),
            (18.9, SpeedLevel::Sl12),
            (19.0, SpeedLevel::Sl13),
            (19.9, SpeedLevel::Sl13),
            (20.0, SpeedLevel::Sl14),
            (30.0, SpeedLevel::Sl14),
        ];

        for (snr, expected_level) in cases {
            let (level, _) = recommend_hf_level(&profile, "hpx_hf", snr);
            assert_eq!(level, expected_level, "snr={snr}");
        }
    }

    #[test]
    fn ofdm_hf_profile_recommends_ofdm_modes() {
        let profile = SessionProfile::by_name("hpx_ofdm_hf").expect("ofdm-hf resolves");
        // Below the lowest rung's floor → floor at the most robust defined rung (SL5),
        // not an unmapped SL2.
        let (low, _) = recommend_hf_level(&profile, "hpx_ofdm_hf", 0.0);
        assert_eq!(low, SpeedLevel::Sl5);
        assert_eq!(profile.mode_for(low), Some("OFDM16"));
        // High SNR → the densest OFDM rung.
        let (high, _) = recommend_hf_level(&profile, "hpx_ofdm_hf", 30.0);
        assert_eq!(high, SpeedLevel::Sl10);
        assert_eq!(profile.mode_for(high), Some("OFDM52-64QAM"));
    }

    #[test]
    fn explicit_override_takes_precedence() {
        assert_eq!(resolve_profile_name(Some("hpx_ofdm_hf")), "hpx_ofdm_hf");
    }
}
