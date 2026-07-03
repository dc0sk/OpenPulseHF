//! Application state, update loop, and theme wiring for the iced operator panel (REQ-UX-04).
//!
//! This is the first scaffold increment: it renders the fixed vertical stack (spectrum → waterfall
//! → ladder → additional info → controls) with selectable Dark/Light/Contrast/System themes. The
//! panel is not yet wired to the daemon; it shows synthetic demo data so the look&feel and theme
//! switching can be exercised. The daemon connection replaces `demo_*` in a later increment.

use std::time::Duration;

use iced::{Subscription, Task, Theme};

use crate::theme::{role_rgb, shade_rgb, ColorRole, EffectiveTheme, Shade, ThemeMode};
use crate::ui;

/// Number of spectrum/waterfall bins.
pub const BINS: usize = 240;
/// Number of waterfall history rows (newest first).
pub const WF_ROWS: usize = 48;
/// Speed-level rungs shown on the ladder (SL1..=SLN).
pub const LADDER_RUNGS: u8 = 12;

/// Top-level panel application state.
pub struct App {
    /// Selected theme (Dark/Light/Contrast/System).
    pub theme_mode: ThemeMode,
    /// Detected OS dark preference, used to resolve the `System` theme.
    pub system_is_dark: bool,

    // --- synthetic demo state (placeholder until the daemon is wired in) ---
    pub tick: u32,
    /// Latest spectrum trace, dBm per bin.
    pub spectrum: Vec<f32>,
    /// Waterfall rows, newest first, dBm per bin.
    pub waterfall: Vec<Vec<f32>>,
    /// Current ladder rung (1..=LADDER_RUNGS).
    pub current_sl: u8,
    /// Current mode label.
    pub mode: &'static str,
    /// Estimated SNR (dB).
    pub snr_db: f32,
    /// Session state label.
    pub state: &'static str,
    /// Transmit indicator.
    pub tx: bool,
    /// Connection indicator.
    pub connected: bool,
}

/// UI messages.
#[derive(Debug, Clone, Copy)]
pub enum Message {
    /// Cycle Dark → Light → Contrast → System.
    ToggleTheme,
    /// Animation/refresh tick.
    Tick,
    /// Toggle the (demo) transmit indicator.
    ToggleTx,
    /// Toggle the (demo) connection.
    ToggleConnect,
}

impl App {
    /// Construct the app and its initial task.
    pub fn new() -> (Self, Task<Message>) {
        let mut app = App {
            theme_mode: ThemeMode::default(),
            system_is_dark: detect_system_dark(),
            tick: 0,
            spectrum: synth_spectrum(0),
            waterfall: vec![vec![-110.0; BINS]; WF_ROWS],
            current_sl: 5,
            mode: "QPSK500",
            snr_db: 14.0,
            state: "Idle",
            tx: false,
            connected: false,
        };
        // Seed a few waterfall rows so it isn't blank on first paint.
        for t in 0..WF_ROWS as u32 {
            app.waterfall[t as usize] = synth_spectrum(t);
        }
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleTheme => self.theme_mode = self.theme_mode.next(),
            Message::Tick => {
                self.tick = self.tick.wrapping_add(1);
                self.spectrum = synth_spectrum(self.tick);
                self.waterfall.insert(0, self.spectrum.clone());
                self.waterfall.truncate(WF_ROWS);
                if self.connected {
                    // Gently wander the demo readouts so the ladder/info feel live.
                    self.snr_db = 14.0 + 6.0 * ((self.tick as f32) * 0.05).sin();
                    self.current_sl = 3 + ((self.snr_db as i32 - 8).clamp(0, 8) as u8);
                    self.current_sl = self.current_sl.clamp(1, LADDER_RUNGS);
                }
            }
            Message::ToggleTx => self.tx = !self.tx,
            Message::ToggleConnect => {
                self.connected = !self.connected;
                self.state = if self.connected { "Connected" } else { "Idle" };
                if !self.connected {
                    self.tx = false;
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(120)).map(|_| Message::Tick)
    }

    /// The concrete theme resolved for this frame (`System` → OS preference).
    pub fn effective_theme(&self) -> EffectiveTheme {
        self.theme_mode.effective(self.system_is_dark)
    }

    /// The iced base theme: background + semantic accents from the active palette, so default
    /// widget chrome and text colours match the stack's styled surfaces.
    pub fn theme(&self) -> Theme {
        let eff = self.effective_theme();
        let c = |rgb: (u8, u8, u8)| iced::Color::from_rgb8(rgb.0, rgb.1, rgb.2);
        Theme::custom(
            format!("OpenPulse {}", self.theme_mode.label()),
            iced::theme::Palette {
                background: c(shade_rgb(eff, Shade::Bg)),
                text: c(role_rgb(eff, ColorRole::RxValue)),
                primary: c(role_rgb(eff, ColorRole::Signal)),
                success: c(role_rgb(eff, ColorRole::Locked)),
                danger: c(role_rgb(eff, ColorRole::TxActive)),
            },
        )
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        ui::view(self)
    }
}

/// A synthetic spectrum trace (dBm per bin): a noise floor plus a couple of moving signal humps —
/// stand-in demo data so the panel renders and the themes can be compared until the daemon is wired.
fn synth_spectrum(tick: u32) -> Vec<f32> {
    let t = tick as f32;
    (0..BINS)
        .map(|i| {
            let x = i as f32 / BINS as f32;
            // Deterministic pseudo-noise floor.
            let n = (((i as u32).wrapping_mul(2654435761).wrapping_add(tick)) >> 24) as f32 / 255.0;
            let floor = -112.0 + n * 6.0;
            // Two humps that drift across the window.
            let c1 = 0.30 + 0.05 * (t * 0.03).sin();
            let c2 = 0.68 + 0.04 * (t * 0.021 + 1.0).cos();
            let hump =
                |c: f32, amp: f32, w: f32| amp * (-((x - c) * (x - c)) / (2.0 * w * w)).exp();
            floor + hump(c1, 70.0, 0.02) + hump(c2, 55.0, 0.035)
        })
        .collect()
}

/// Best-effort detection of the OS dark/light preference for the `System` theme. Defaults to dark
/// when it can't tell (matches the panel's default).
pub fn detect_system_dark() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if s.contains("dark") {
                return true;
            }
            if s.contains("light") {
                return false;
            }
        }
    }
    true
}
