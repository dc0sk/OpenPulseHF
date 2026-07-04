//! Theme foundation for the iced operator panel (REQ-UX-04).
//!
//! Modeled on the K4remote look&feel: a small set of semantic **colour roles** and layered
//! **surface shades**, resolved per selectable **theme** — Dark, Light, Contrast, and System (which
//! follows the OS light/dark preference). The core here is framework-agnostic (plain sRGB triples)
//! so the palette logic is unit-testable without iced; the iced `Color` mapping lives in `ui.rs`.

/// Semantic colour role — *meaning*, not a literal colour. The active theme maps each role to an
/// sRGB triple. The set is tuned for the modem panel (spectrum/waterfall/ladder/info/controls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    /// Signal / primary accent — spectrum trace, selected controls, RX readouts (blue).
    Signal,
    /// Transmit state and TX-side values (amber).
    TxActive,
    /// Locked / good / the active ladder rung (green).
    Locked,
    /// Primary readout text (near-white on dark grounds).
    RxValue,
    /// Caution, e.g. weak margin / retransmit (yellow).
    Caution,
    /// An off / available / inactive control (dim grey).
    Inactive,
}

/// Layered surface shades: window background, grouping panels, recessed wells, and interactive
/// controls step in luminance so depth reads without heavy chrome. Strict luminance ordering
/// (`Bg` darkest → `ControlHover` lightest on dark themes) is the testable contract; `Edge` is the
/// hairline border and is exempt (it inverts on the Contrast theme).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    /// Window background.
    Bg,
    /// Grouping panel behind a band of related controls (a stack section).
    Panel,
    /// Recessed well: spectrum/waterfall margins, meter tracks.
    Track,
    /// Interactive control (button) at rest.
    Control,
    /// Control under the pointer.
    ControlHover,
    /// Hairline border / edge.
    Edge,
}

/// Selectable UI theme (REQ-UX-04). `System` follows the OS light/dark preference; the other three
/// are explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Dark theme (the default).
    #[default]
    Dark,
    /// Light theme for bright environments.
    Light,
    /// High-contrast theme (pure black/white grounds, bright accents).
    Contrast,
    /// Follow the operating system's light/dark preference.
    System,
}

/// A concrete theme actually used to resolve colours — `System` resolves to one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveTheme {
    Dark,
    Light,
    Contrast,
}

impl ThemeMode {
    /// Cycle order for the toggle button: Dark → Light → Contrast → System → …
    pub fn next(self) -> Self {
        match self {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Contrast,
            ThemeMode::Contrast => ThemeMode::System,
            ThemeMode::System => ThemeMode::Dark,
        }
    }

    /// Button label for the current mode.
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Dark => "Dark",
            ThemeMode::Light => "Light",
            ThemeMode::Contrast => "Contrast",
            ThemeMode::System => "System",
        }
    }

    /// Resolve to a concrete palette; `System` uses the detected OS preference.
    pub fn effective(self, system_is_dark: bool) -> EffectiveTheme {
        match self {
            ThemeMode::Dark => EffectiveTheme::Dark,
            ThemeMode::Light => EffectiveTheme::Light,
            ThemeMode::Contrast => EffectiveTheme::Contrast,
            ThemeMode::System => {
                if system_is_dark {
                    EffectiveTheme::Dark
                } else {
                    EffectiveTheme::Light
                }
            }
        }
    }
}

/// Surface shade → sRGB for a given theme.
pub fn shade_rgb(theme: EffectiveTheme, s: Shade) -> (u8, u8, u8) {
    match theme {
        EffectiveTheme::Dark => match s {
            Shade::Bg => (0x0B, 0x0D, 0x10),
            Shade::Panel => (0x14, 0x17, 0x1B),
            Shade::Track => (0x1A, 0x1D, 0x22),
            Shade::Control => (0x24, 0x28, 0x2E),
            Shade::ControlHover => (0x2F, 0x34, 0x3B),
            Shade::Edge => (0x3A, 0x40, 0x48),
        },
        EffectiveTheme::Light => match s {
            Shade::Bg => (0xEE, 0xF1, 0xF4),
            Shade::Panel => (0xFF, 0xFF, 0xFF),
            Shade::Track => (0xE4, 0xE8, 0xED),
            Shade::Control => (0xE8, 0xEC, 0xF1),
            Shade::ControlHover => (0xDA, 0xDF, 0xE6),
            Shade::Edge => (0xC6, 0xCD, 0xD5),
        },
        EffectiveTheme::Contrast => match s {
            Shade::Bg => (0x00, 0x00, 0x00),
            Shade::Panel => (0x0A, 0x0A, 0x0A),
            Shade::Track => (0x14, 0x14, 0x14),
            Shade::Control => (0x1E, 0x1E, 0x1E),
            Shade::ControlHover => (0x30, 0x30, 0x30),
            Shade::Edge => (0xFF, 0xFF, 0xFF),
        },
    }
}

/// Semantic role → sRGB for a given theme. Light darkens accents for contrast on a light ground;
/// Contrast brightens them against pure black.
pub fn role_rgb(theme: EffectiveTheme, role: ColorRole) -> (u8, u8, u8) {
    match theme {
        EffectiveTheme::Dark => match role {
            ColorRole::Signal => (0x3D, 0x9B, 0xFF),
            ColorRole::TxActive => (0xFF, 0x9A, 0x1E),
            ColorRole::Locked => (0x33, 0xCC, 0x66),
            ColorRole::RxValue => (0xEC, 0xEF, 0xF2),
            ColorRole::Caution => (0xFF, 0xD4, 0x33),
            ColorRole::Inactive => (0x66, 0x6B, 0x72),
        },
        EffectiveTheme::Light => match role {
            ColorRole::Signal => (0x1E, 0x66, 0xD0),
            ColorRole::TxActive => (0xC7, 0x6A, 0x00),
            ColorRole::Locked => (0x1E, 0x8A, 0x44),
            ColorRole::RxValue => (0x1A, 0x1E, 0x24),
            ColorRole::Caution => (0xB8, 0x86, 0x00),
            ColorRole::Inactive => (0x7A, 0x80, 0x88),
        },
        EffectiveTheme::Contrast => match role {
            ColorRole::Signal => (0x4D, 0xB1, 0xFF),
            ColorRole::TxActive => (0xFF, 0xB0, 0x2E),
            ColorRole::Locked => (0x3D, 0xF0, 0x7A),
            ColorRole::RxValue => (0xFF, 0xFF, 0xFF),
            ColorRole::Caution => (0xFF, 0xEE, 0x00),
            ColorRole::Inactive => (0xB0, 0xB0, 0xB0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma((r, g, b): (u8, u8, u8)) -> f32 {
        0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
    }

    #[test]
    fn theme_cycle_visits_all_four_and_returns() {
        let mut m = ThemeMode::Dark;
        let seen: Vec<_> = (0..4)
            .map(|_| {
                let cur = m;
                m = m.next();
                cur
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ThemeMode::Dark,
                ThemeMode::Light,
                ThemeMode::Contrast,
                ThemeMode::System
            ]
        );
        assert_eq!(m, ThemeMode::Dark, "cycle must return to the start");
    }

    #[test]
    fn system_resolves_to_os_preference() {
        assert_eq!(ThemeMode::System.effective(true), EffectiveTheme::Dark);
        assert_eq!(ThemeMode::System.effective(false), EffectiveTheme::Light);
        // Explicit modes ignore the OS preference.
        assert_eq!(ThemeMode::Dark.effective(false), EffectiveTheme::Dark);
        assert_eq!(ThemeMode::Light.effective(true), EffectiveTheme::Light);
        assert_eq!(
            ThemeMode::Contrast.effective(true),
            EffectiveTheme::Contrast
        );
    }

    #[test]
    fn surface_shades_are_luminance_ordered() {
        // Bg → ControlHover must strictly increase (dark/contrast) or decrease (light) in luminance,
        // so depth reads consistently; Edge (hairline) is exempt.
        for theme in [EffectiveTheme::Dark, EffectiveTheme::Contrast] {
            let steps = [
                Shade::Bg,
                Shade::Panel,
                Shade::Track,
                Shade::Control,
                Shade::ControlHover,
            ];
            for w in steps.windows(2) {
                assert!(
                    luma(shade_rgb(theme, w[0])) < luma(shade_rgb(theme, w[1])),
                    "{theme:?}: {:?} should be darker than {:?}",
                    w[0],
                    w[1]
                );
            }
        }
        // Light theme: background is light, panel (white) is the brightest ground.
        assert!(luma(shade_rgb(EffectiveTheme::Light, Shade::Bg)) > 200.0);
        assert!(
            luma(shade_rgb(EffectiveTheme::Light, Shade::Panel))
                >= luma(shade_rgb(EffectiveTheme::Light, Shade::Bg))
        );
    }

    #[test]
    fn roles_are_distinct_in_every_theme() {
        let roles = [
            ColorRole::Signal,
            ColorRole::TxActive,
            ColorRole::Locked,
            ColorRole::RxValue,
            ColorRole::Caution,
            ColorRole::Inactive,
        ];
        for theme in [
            EffectiveTheme::Dark,
            EffectiveTheme::Light,
            EffectiveTheme::Contrast,
        ] {
            for (i, &a) in roles.iter().enumerate() {
                for &b in &roles[i + 1..] {
                    assert_ne!(
                        role_rgb(theme, a),
                        role_rgb(theme, b),
                        "{theme:?}: {a:?} and {b:?} must be distinguishable"
                    );
                }
            }
        }
    }

    #[test]
    fn readout_text_contrasts_with_background_in_every_theme() {
        for theme in [
            EffectiveTheme::Dark,
            EffectiveTheme::Light,
            EffectiveTheme::Contrast,
        ] {
            let bg = luma(shade_rgb(theme, Shade::Bg));
            let text = luma(role_rgb(theme, ColorRole::RxValue));
            assert!(
                (bg - text).abs() > 90.0,
                "{theme:?}: readout text must contrast with the background"
            );
        }
    }
}
