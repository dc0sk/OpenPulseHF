//! The panel view (REQ-UX-04), rendered from the live `PanelState` with the active theme's palette:
//! a controls band on top, then spectrum → waterfall → ladder, and a tabbed lower panel
//! (Additional info / Daemon config / Messages / Event log).

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, container, pick_list, scrollable, slider, text_input, tooltip, Button, Column,
    Container, Row, Space, Text,
};
use iced::{
    mouse, Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size,
    Theme,
};

use crate::app::{App, Message, Tab, LADDER_RUNGS};
use crate::connection::TransportKind;
use crate::state::RigSnapshot;
use crate::theme::{role_rgb, shade_rgb, ColorRole, EffectiveTheme, Shade};

/// Modem modes selectable from the panel (mirrors the egui panel's list).
const MODES: &[&str] = &[
    "BPSK31",
    "BPSK63",
    "BPSK100",
    "BPSK250",
    "BPSK250-RRC",
    "QPSK125",
    "QPSK250",
    "QPSK500",
    "QPSK500-RRC",
    "QPSK1000",
    "QPSK1000-HF",
    "QPSK1000-HF-RRC",
    "QPSK1000-RRC",
    "QPSK2000",
    "QPSK2000-RRC",
    "QPSK9600",
    "QPSK9600-RRC",
    "8PSK500",
    "8PSK500-RRC",
    "8PSK1000",
    "8PSK1000-HF",
    "8PSK1000-HF-RRC",
    "8PSK1000-RRC",
    "8PSK2000",
    "8PSK2000-RRC",
    "8PSK9600",
    "8PSK9600-RRC",
    "64QAM500",
    "64QAM1000",
    "64QAM2000-RRC",
    "OFDM16",
    "OFDM52",
    "OFDM52-8PSK",
    "OFDM52-16QAM",
    "OFDM52-32QAM",
    "OFDM52-64QAM",
    "SCFDMA16",
    "SCFDMA52",
    "SCFDMA52-8PSK",
    "SCFDMA52-16QAM",
    "SCFDMA52-32QAM",
    "SCFDMA52-64QAM",
    "SCFDMA52-64QAM-P4",
    "SCFDMA26-8PSK",
    "SCFDMA26-16QAM",
    "SCFDMA26-32QAM",
    "PILOT-QPSK500",
    "PILOT-8PSK500",
    "PILOT-16QAM500",
    "PILOT-32APSK500",
    "PILOT-QPSK500-RRC",
    "PILOT-8PSK500-RRC",
    "PILOT-16QAM500-RRC",
    "PILOT-32APSK500-RRC",
];

/// OTA adaptive-session profiles (`SessionProfile::by_name`).
const PROFILES: &[&str] = &[
    "hpx500",
    "hpx_hf",
    "hpx_ofdm_hf",
    "hpx_narrowband",
    "hpx_narrowband_hd",
    "hpx_wideband",
    "hpx_wideband_hd",
    "hpx_modcod",
    "hpx_pilot",
    "hpx_pilot_rrc",
    "hpx_pilot_fast",
    "hpx_pilot_fast_rrc",
];

// Spectrum window (dBFS): 0 dB at the top down to −120 dB.
const TOP_DB: f32 = 0.0;
const RANGE_DB: f32 = 120.0;

/// Spectrum / waterfall canvas heights.
const SPECTRUM_H: f32 = 140.0;
const WATERFALL_H: f32 = 120.0;
/// Lower tabbed panel height ≈ the spectrum + waterfall panels stacked (their canvases plus each
/// panel's title/padding/border chrome, plus the inter-panel spacing).
const LOWER_PANEL_H: f32 = SPECTRUM_H + WATERFALL_H + 2.0 * 41.0 + 8.0;

/// Best-effort OS dark/light detection for the `System` theme.
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

pub fn shade(eff: EffectiveTheme, s: Shade) -> Color {
    let (r, g, b) = shade_rgb(eff, s);
    Color::from_rgb8(r, g, b)
}
pub fn role(eff: EffectiveTheme, r: ColorRole) -> Color {
    let (rr, gg, bb) = role_rgb(eff, r);
    Color::from_rgb8(rr, gg, bb)
}
fn lerp(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}
fn dbm_to_y(db: f32, h: f32) -> f32 {
    ((TOP_DB - db) / RANGE_DB * h).clamp(0.0, h)
}

/// Wrap an interactive widget in a hover tooltip describing its purpose.
fn tip<'a>(
    el: impl Into<Element<'a, Message>>,
    text: &'static str,
    eff: EffectiveTheme,
) -> Element<'a, Message> {
    tooltip(
        el,
        Container::new(
            Text::new(text)
                .size(12)
                .color(role(eff, ColorRole::RxValue)),
        )
        .padding([4, 8])
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::ControlHover))),
            border: Border {
                color: shade(eff, Shade::Edge),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        }),
        tooltip::Position::FollowCursor,
    )
    .gap(6)
    .into()
}

/// Owned snapshot of the shared state, so the view builds its `Element` without holding the lock.
struct Snap {
    connected: bool,
    mode: String,
    speed_level_num: u8,
    hpx_state: String,
    afc_hz: f32,
    dcd_busy: bool,
    dcd_energy: f32,
    effective_bps: f32,
    ecc_rate: f32,
    compress_ratio: f32,
    signal_strength_dbm: Option<i32>,
    cpu_percent: f32,
    ram_mb: f32,
    ram_percent: f32,
    gpu_percent: Option<f32>,
    decode_latency_ms: f32,
    ptt_active: bool,
    rf_connected: bool,
    rf_peer: Option<String>,
    repeater_enabled: bool,
    pending_qsy: Option<String>,
    ota_active: bool,
    ota_tx_mode: Option<String>,
    ota_tx_level: Option<String>,
    ota_tx_fec: String,
    ota_rx_recommended: Option<String>,
    ota_is_locked: bool,
    rig_a: Option<RigSnapshot>,
    rig_b: Option<RigSnapshot>,
    spectrum: Vec<f32>,
    waterfall: Vec<Vec<f32>>,
    log: Vec<String>,
    inbox: Vec<crate::state::MessageSummary>,
    open_msg_id: Option<u64>,
    open_msg_body: Option<String>,
    ecc_history: Vec<f32>,
    rx_frames_by_level: Vec<u32>,
    tx_frames_by_level: Vec<u32>,
}

pub fn view(app: &App) -> Element<'_, Message> {
    let eff = app.effective_theme();
    let snap = {
        let st = app.shared.lock().unwrap();
        Snap {
            connected: st.connected,
            mode: st.mode.clone(),
            speed_level_num: st.speed_level_num,
            hpx_state: st.hpx_state.clone(),
            afc_hz: st.afc_hz,
            dcd_busy: st.dcd_busy,
            dcd_energy: st.dcd_energy,
            effective_bps: st.effective_bps,
            ecc_rate: st.ecc_rate,
            compress_ratio: st.compress_ratio,
            signal_strength_dbm: st.signal_strength_dbm,
            cpu_percent: st.cpu_percent,
            ram_mb: st.ram_mb,
            ram_percent: st.ram_percent,
            gpu_percent: st.gpu_percent,
            decode_latency_ms: st.decode_latency_ms,
            ptt_active: st.ptt_active,
            rf_connected: st.rf_connected,
            rf_peer: st.rf_peer.clone(),
            repeater_enabled: st.repeater_enabled,
            pending_qsy: st.pending_qsy_token.clone(),
            ota_active: st.ota_active,
            ota_tx_mode: st.ota_tx_mode.clone(),
            ota_tx_level: st.ota_tx_level.clone(),
            ota_tx_fec: st.ota_tx_fec.clone(),
            ota_rx_recommended: st.ota_rx_recommended_level.clone(),
            ota_is_locked: st.ota_is_locked,
            rig_a: st.rig_a.clone(),
            rig_b: st.rig_b.clone(),
            spectrum: st.spectrum_bins.clone(),
            waterfall: st.spectrum_history.iter().cloned().collect(),
            log: st.event_log.iter().take(60).cloned().collect(),
            inbox: st.inbox.clone(),
            open_msg_id: st.open_message_id,
            open_msg_body: st.open_message_body.clone(),
            ecc_history: st.ecc_history.iter().cloned().collect(),
            rx_frames_by_level: st.rx_frames_by_level.to_vec(),
            tx_frames_by_level: st.tx_frames_by_level.to_vec(),
        }
    };

    let stack = Column::new()
        .spacing(8)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        // Controls band across the top.
        .push(panel(eff, "Controls", controls_widget(app, &snap, eff)))
        .push(panel(eff, "Spectrum", spectrum_widget(&snap, eff)))
        .push(panel(eff, "Waterfall", waterfall_widget(&snap, eff)))
        .push(panel(eff, "Ladder", ladder_widget(&snap, eff)))
        // Additional info / Daemon config / Messages / Event log as one tabbed panel that
        // fills the remaining height, so every tab is the same size.
        .push(tabbed_lower(app, &snap, eff));

    Container::new(stack)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::Bg))),
            ..container::Style::default()
        })
        .into()
}

fn panel<'a>(
    eff: EffectiveTheme,
    title: &'a str,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let body = Column::new()
        .spacing(6)
        .push(
            Text::new(title.to_uppercase())
                .size(11)
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(content);
    Container::new(body)
        .width(Length::Fill)
        .padding(10)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::Panel))),
            border: Border {
                color: shade(eff, Shade::Edge),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn spectrum_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    Canvas::new(SpectrumTrace {
        trace: snap.spectrum.clone(),
        bg: shade(eff, Shade::Track),
        line: role(eff, ColorRole::Signal),
        grid: shade(eff, Shade::Edge),
    })
    .width(Length::Fill)
    .height(Length::Fixed(SPECTRUM_H))
    .into()
}

fn waterfall_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    Canvas::new(Waterfall {
        rows: snap.waterfall.clone(),
        low: shade(eff, Shade::Track),
        mid: role(eff, ColorRole::Signal),
        high: role(eff, ColorRole::Caution),
    })
    .width(Length::Fill)
    .height(Length::Fixed(WATERFALL_H))
    .into()
}

fn ladder_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    // HPX state chips + the SL rung ladder.
    let states = [
        "Idle",
        "Discovery",
        "Training",
        "ActiveTransfer",
        "Recovery",
        "RelayActive",
        "Teardown",
        "Failed",
    ];
    let mut hpx = Row::new().spacing(4).align_y(Alignment::Center);
    for s in states {
        let active = snap.hpx_state == s;
        hpx = hpx.push(state_chip(eff, s, active, ColorRole::Signal));
    }
    let mut rungs = Row::new().spacing(3).align_y(Alignment::Center);
    for sl in 1..=LADDER_RUNGS {
        let (bg, fg) = if sl == snap.speed_level_num {
            (role(eff, ColorRole::Locked), shade(eff, Shade::Bg))
        } else if sl < snap.speed_level_num {
            (shade(eff, Shade::Control), role(eff, ColorRole::Signal))
        } else {
            (shade(eff, Shade::Track), role(eff, ColorRole::Inactive))
        };
        rungs = rungs.push(chip(eff, &format!("{sl}"), bg, fg));
    }
    Column::new()
        .spacing(8)
        .push(hpx)
        .push(scrollable(rungs).direction(scrollable::Direction::Horizontal(Default::default())))
        .into()
}

fn info_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    // Column 1 — session.
    let col1 = Column::new()
        .spacing(4)
        .push(col_title(eff, "Session"))
        .push(info_row(eff, "Mode", &snap.mode, ColorRole::Signal))
        .push(info_row(
            eff,
            "Speed",
            &format!("SL{}", snap.speed_level_num),
            ColorRole::Locked,
        ))
        .push(info_row(eff, "HPX", &snap.hpx_state, ColorRole::RxValue))
        .push(info_row(
            eff,
            "Eff. bps",
            &format!("{:.0}", snap.effective_bps),
            ColorRole::RxValue,
        ))
        .push(info_row(
            eff,
            "ECC rate",
            &format!("{:.2} %", snap.ecc_rate * 100.0),
            ColorRole::RxValue,
        ))
        .push(info_row(
            eff,
            "Compress",
            &format_compression(snap.compress_ratio),
            ColorRole::RxValue,
        ));

    // Column 2 — signal.
    let mut col2 = Column::new()
        .spacing(4)
        .push(col_title(eff, "Signal"))
        .push(info_row(
            eff,
            "AFC",
            &format!("{:+.1} Hz", snap.afc_hz),
            ColorRole::RxValue,
        ))
        .push(info_row(
            eff,
            "DCD",
            if snap.dcd_busy { "BUSY" } else { "clear" },
            if snap.dcd_busy {
                ColorRole::TxActive
            } else {
                ColorRole::Inactive
            },
        ))
        .push(bar_row(
            eff,
            "DCD energy",
            (snap.dcd_energy * 20.0).clamp(0.0, 1.0),
            ColorRole::Signal,
        ));
    if let Some(dbm) = snap.signal_strength_dbm {
        col2 = col2.push(info_row(
            eff,
            "S-meter",
            &format!("{dbm} dBm"),
            ColorRole::Signal,
        ));
    }

    // Column 3 — resources + rigs.
    let mut col3 = Column::new()
        .spacing(4)
        .push(col_title(eff, "Resources"))
        .push(bar_row(
            eff,
            "CPU",
            snap.cpu_percent / 100.0,
            heat_role(snap.cpu_percent),
        ))
        .push(bar_row(
            eff,
            &format!("RAM {:.0}M", snap.ram_mb),
            snap.ram_percent / 100.0,
            ColorRole::Signal,
        ));
    if let Some(g) = snap.gpu_percent {
        col3 = col3.push(bar_row(eff, "GPU", g / 100.0, heat_role(g)));
    }
    col3 = col3.push(info_row(
        eff,
        "Decode",
        &format!("{:.1} ms", snap.decode_latency_ms),
        ColorRole::RxValue,
    ));
    if let Some(r) = &snap.rig_a {
        col3 = col3.push(info_row(eff, "Rig A", &fmt_rig(r), ColorRole::RxValue));
    }
    if let Some(r) = &snap.rig_b {
        col3 = col3.push(info_row(eff, "Rig B", &fmt_rig(r), ColorRole::RxValue));
    }

    let columns = Row::new()
        .spacing(24)
        .width(Length::Fill)
        .push(col1.width(Length::FillPortion(1)))
        .push(col2.width(Length::FillPortion(1)))
        .push(col3.width(Length::FillPortion(1)));

    Column::new()
        .spacing(8)
        .push(columns)
        // ECC-rate trend (rolling ~2 min), full width below the columns.
        .push(
            Text::new("ECC rate (2 min)")
                .size(11)
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(
            Canvas::new(EccTrend {
                hist: snap.ecc_history.clone(),
                bg: shade(eff, Shade::Track),
                line: role(eff, ColorRole::Caution),
                grid: shade(eff, Shade::Edge),
            })
            .width(Length::Fill)
            .height(Length::Fixed(56.0)),
        )
        .into()
}

/// Small uppercase heading for an info column.
fn col_title(eff: EffectiveTheme, label: &str) -> Element<'static, Message> {
    Text::new(label.to_uppercase())
        .size(10)
        .color(role(eff, ColorRole::Signal))
        .into()
}

fn controls_widget(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let dot = if snap.connected {
        ColorRole::Locked
    } else {
        ColorRole::Inactive
    };
    // Connection row.
    let transports: &[&str] = &["TCP", "WS"];
    let tsel = if matches!(app.transport_kind, TransportKind::WebSocket) {
        "WS"
    } else {
        "TCP"
    };
    // RF peer — sits on the connection row, right of Connect.
    let peer = if snap.rf_connected {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                Text::new(format!("RF: {}", snap.rf_peer.clone().unwrap_or_default()))
                    .size(13)
                    .color(role(eff, ColorRole::Locked)),
            )
            .push(accent_btn(
                eff,
                "Disconnect RF",
                Message::DisconnectPeer,
                ColorRole::TxActive,
                "Tear down the current RF peer link",
            ))
    } else {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(tip(
                text_input("CALLSIGN", &app.peer_call)
                    .on_input(Message::PeerCallChanged)
                    .size(13)
                    .width(Length::Fixed(110.0)),
                "Peer callsign to call over RF",
                eff,
            ))
            .push(accent_btn(
                eff,
                "Connect RF",
                Message::ConnectPeer,
                ColorRole::Locked,
                "Call the entered peer over RF via the TNC",
            ))
    };
    let conn = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(tip(
            pick_list(transports, Some(tsel), |s: &str| {
                Message::SelectTransport(s == "WS")
            })
            .text_size(13),
            "Control-port transport: raw TCP or WebSocket",
            eff,
        ))
        .push(tip(
            text_input("host:port", &app.addr)
                .on_input(Message::AddrChanged)
                .size(13)
                .width(Length::Fixed(150.0)),
            "Daemon control-port address (host:port for TCP, ws://… for WS)",
            eff,
        ))
        .push(accent_btn(
            eff,
            if app.is_connected() {
                "Disconnect"
            } else {
                "Connect"
            },
            Message::ConnectToggle,
            if app.is_connected() {
                ColorRole::TxActive
            } else {
                ColorRole::Locked
            },
            "Connect to / disconnect from the daemon control port",
        ))
        .push(peer)
        .push(Text::new("●").size(13).color(role(eff, dot)))
        .push(Space::with_width(Length::Fill))
        .push(accent_btn(
            eff,
            if snap.ptt_active { "PTT ●" } else { "PTT" },
            Message::Ptt,
            if snap.ptt_active {
                ColorRole::TxActive
            } else {
                ColorRole::Inactive
            },
            "Key / unkey the transmitter (PTT)",
        ));

    // Mode + frequency.
    let sel = MODES.iter().copied().find(|&m| m == app.mode_sel.as_str());
    let mode_freq = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(tip(
            pick_list(MODES, sel, |m: &str| Message::ModeSelected(m.to_string())).text_size(13),
            "Set the active modem mode",
            eff,
        ))
        .push(tip(
            text_input("kHz", &app.freq_khz)
                .on_input(Message::FreqChanged)
                .size(13)
                .width(Length::Fixed(90.0)),
            "Frequency in kHz; press Tune to apply",
            eff,
        ))
        .push(neutral_btn(
            eff,
            "Tune",
            Message::TuneFreq,
            "Set the rig frequency (kHz) via CAT / rigctld",
        ));

    // Feature toggles (Repeater lives in the Config panel now).
    let toggles = Row::new()
        .spacing(6)
        .push(toggle_btn(
            eff,
            "CE-SSB",
            app.cessb_on,
            Message::ToggleCessb,
            "CE-SSB TX envelope conditioning (multicarrier modes only)",
        ))
        .push(toggle_btn(
            eff,
            "Notch",
            app.notch_on,
            Message::ToggleNotch,
            "Receiver auto-notch: removes out-of-band CW interference before demod",
        ))
        .push(toggle_btn(
            eff,
            "AGC",
            app.agc_on,
            Message::ToggleAgc,
            "Receiver streaming AGC: normalises capture level before demod",
        ))
        .push(toggle_btn(
            eff,
            "Logbook",
            app.logbook_on,
            Message::ToggleLogbook,
            "Automatic ADIF logbook: one record per connect → disconnect",
        ))
        .push(toggle_btn(
            eff,
            "QSY",
            app.config_draft.qsy_enabled,
            Message::CfgQsy(!app.config_draft.qsy_enabled),
            "Enable the QSY frequency-agility protocol (applied via Config → Apply)",
        ))
        .push(toggle_btn(
            eff,
            "Tune@SWR",
            app.config_draft.allow_tuner_on_high_swr,
            Message::CfgTuneSwr(!app.config_draft.allow_tuner_on_high_swr),
            "Allow the tuner to operate at high SWR (applied via Config → Apply)",
        ));

    // Sliders: TX attenuation and squelch, side by side.
    let sliders = Row::new()
        .spacing(16)
        .align_y(Alignment::Center)
        .push(slider_row(
            eff,
            &format!("TX atten {:.1} dB", app.tx_atten_db),
            -30.0..=0.0,
            app.tx_atten_db,
            Message::AttenChanged,
            "Transmit attenuation for the current band (dB)",
        ))
        .push(slider_row(
            eff,
            &format!("Squelch {:.3}", app.squelch),
            0.0..=0.2,
            app.squelch,
            Message::SquelchChanged,
            "DCD / squelch RMS threshold — raise to clear a noisy band floor",
        ));

    // OTA.
    let ota = if snap.ota_active {
        let status = format!(
            "OTA {} {}/{} (rec {})",
            snap.ota_tx_level.clone().unwrap_or_else(|| "—".into()),
            snap.ota_tx_mode.clone().unwrap_or_else(|| "—".into()),
            snap.ota_tx_fec,
            snap.ota_rx_recommended
                .clone()
                .unwrap_or_else(|| "—".into()),
        );
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                Text::new(status)
                    .size(12)
                    .color(role(eff, ColorRole::Signal)),
            )
            .push(neutral_btn(
                eff,
                if snap.ota_is_locked { "Unlock" } else { "Lock" },
                Message::OtaLockToggle,
                "Lock the OTA ladder to the current level, or release it to adapt",
            ))
            .push(accent_btn(
                eff,
                "Stop",
                Message::StopOta,
                ColorRole::TxActive,
                "Stop the active OTA adaptive session",
            ))
    } else {
        let prof_sel = PROFILES
            .iter()
            .copied()
            .find(|&p| p == app.ota_profile.as_str());
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(tip(
                pick_list(PROFILES, prof_sel, |p: &str| {
                    Message::OtaProfileChanged(p.to_string())
                })
                .text_size(13),
                "OTA adaptive-session profile (rate ladder)",
                eff,
            ))
            .push(accent_btn(
                eff,
                "Start OTA",
                Message::StartOta,
                ColorRole::Locked,
                "Start a receiver-led OTA adaptive session with the chosen profile",
            ))
    };

    // Line 1: connection + PTT, then mode/frequency, on one horizontal band.
    let line1 = Row::new()
        .spacing(12)
        .align_y(Alignment::Center)
        .push(conn)
        .push(mode_freq);

    // Line 2: feature toggles, then the theme toggle on the right.
    let line2 = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(toggles)
        .push(Space::with_width(Length::Fill))
        .push(neutral_btn(
            eff,
            &format!("Theme: {}", app.theme_mode.label()),
            Message::ToggleTheme,
            "Cycle the theme: Dark → Light → Contrast → System",
        ));

    // Line 3: sliders side by side, then OTA.
    let line3 = Row::new()
        .spacing(16)
        .align_y(Alignment::Center)
        .push(sliders)
        .push(Space::with_width(Length::Fill))
        .push(ota);

    let mut col = Column::new().spacing(8).push(line1).push(line2).push(line3);

    // QSY decision (only when a proposal is pending).
    if snap.pending_qsy.is_some() {
        col = col.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    Text::new("QSY pending")
                        .size(13)
                        .color(role(eff, ColorRole::Caution)),
                )
                .push(accent_btn(
                    eff,
                    "Accept",
                    Message::AcceptQsy,
                    ColorRole::Locked,
                    "Accept the pending QSY frequency proposal",
                ))
                .push(accent_btn(
                    eff,
                    "Reject",
                    Message::RejectQsy,
                    ColorRole::TxActive,
                    "Reject the pending QSY frequency proposal",
                )),
        );
    }
    col.into()
}

/// Additional info / Daemon config / Messages / Event log as one tabbed panel.
fn tabbed_lower(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let tab_btn = |label: &str, tab: Tab, tip_text: &'static str| -> Element<'static, Message> {
        let active = app.active_tab == tab;
        let (bg, fg) = if active {
            (role(eff, ColorRole::Signal), shade(eff, Shade::Bg))
        } else {
            (shade(eff, Shade::Control), role(eff, ColorRole::Inactive))
        };
        let btn = Button::new(Text::new(label.to_string()).size(12).color(fg))
            .padding([4, 12])
            .on_press(Message::SelectTab(tab))
            .style(move |_t: &Theme, _s: button::Status| button::Style {
                background: Some(Background::Color(bg)),
                text_color: fg,
                border: Border {
                    color: shade(eff, Shade::Edge),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..button::Style::default()
            });
        tip(btn, tip_text, eff)
    };
    let header = Row::new()
        .spacing(4)
        .push(tab_btn(
            "Additional info",
            Tab::Info,
            "Live session, signal, and resource readouts",
        ))
        .push(tab_btn(
            "Statistics",
            Tab::Stats,
            "Successfully transferred frames per ladder step this session",
        ))
        .push(tab_btn(
            "Daemon config",
            Tab::Config,
            "View and edit the daemon configuration",
        ))
        .push(tab_btn(
            "Messages",
            Tab::Messages,
            "Inbox and message composer",
        ))
        .push(tab_btn(
            "Event log",
            Tab::Log,
            "Scrolling log of engine and session events",
        ));
    let content = match app.active_tab {
        Tab::Info => info_widget(snap, eff),
        Tab::Stats => stats_widget(snap, eff),
        Tab::Config => config_widget(app, snap, eff),
        Tab::Messages => messages_widget(app, snap, eff),
        Tab::Log => log_widget(snap, eff),
    };
    // The content box fills the panel, so every tab renders at the same height.
    let content_box = Container::new(content)
        .width(Length::Fill)
        .height(Length::Fill);
    let body = Column::new().spacing(8).push(header).push(content_box);
    // Fixed height ≈ the spectrum + waterfall panels stacked.
    Container::new(body)
        .width(Length::Fill)
        .height(Length::Fixed(LOWER_PANEL_H))
        .padding(10)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::Panel))),
            border: Border {
                color: shade(eff, Shade::Edge),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn messages_widget(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let header = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(
            Text::new(format!("Inbox ({})", snap.inbox.len()))
                .size(13)
                .color(role(eff, ColorRole::RxValue)),
        )
        .push(neutral_btn(
            eff,
            "Refresh",
            Message::RefreshInbox,
            "Reload the message inbox from the daemon",
        ));

    let mut list = Column::new().spacing(3);
    for m in &snap.inbox {
        let open = snap.open_msg_id == Some(m.id);
        let label = format!("{} {} — {}", hhmm(m.timestamp_secs), m.from, m.subject);
        let accent = if open {
            ColorRole::Signal
        } else {
            ColorRole::Inactive
        };
        let row = Row::new()
            .spacing(6)
            .align_y(Alignment::Center)
            .push(link_btn(
                eff,
                &label,
                Message::OpenMsg(m.id),
                accent,
                "Open this message",
            ))
            .push(Space::with_width(Length::Fill))
            .push(link_btn(
                eff,
                "✕",
                Message::DeleteMsg(m.id),
                ColorRole::TxActive,
                "Delete this message",
            ));
        list = list.push(row);
    }
    let inbox = scrollable(list).height(Length::Fixed(120.0));

    let reader: Element<Message> = match &snap.open_msg_body {
        Some(body) => Container::new(
            Text::new(body.clone())
                .size(12)
                .color(role(eff, ColorRole::RxValue)),
        )
        .padding(8)
        .width(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::Track))),
            border: Border {
                color: shade(eff, Shade::Edge),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into(),
        None => Space::with_height(Length::Fixed(0.0)).into(),
    };

    let can_send = !app.msg_to.trim().is_empty()
        && !app.msg_subject.trim().is_empty()
        && !app.msg_body.trim().is_empty();
    let send = accent_btn(
        eff,
        "Send",
        Message::SendMsg,
        if can_send {
            ColorRole::Locked
        } else {
            ColorRole::Inactive
        },
        "Send the composed message (needs To, Subject, and Body)",
    );
    let compose = Column::new()
        .spacing(5)
        .push(
            Text::new("Compose")
                .size(12)
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(
            Row::new()
                .spacing(6)
                .push(tip(
                    text_input("CALLSIGN", &app.msg_to)
                        .on_input(Message::MsgTo)
                        .size(13)
                        .width(Length::Fixed(120.0)),
                    "Recipient callsign",
                    eff,
                ))
                .push(tip(
                    text_input("subject", &app.msg_subject)
                        .on_input(Message::MsgSubject)
                        .size(13)
                        .width(Length::Fill),
                    "Message subject",
                    eff,
                )),
        )
        .push(tip(
            text_input("Message body…", &app.msg_body)
                .on_input(Message::MsgBody)
                .size(13),
            "Message body",
            eff,
        ))
        .push(Row::new().push(Space::with_width(Length::Fill)).push(send));

    Column::new()
        .spacing(8)
        .push(header)
        .push(inbox)
        .push(reader)
        .push(compose)
        .into()
}

fn config_widget(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let c = &app.config_draft;
    let bandplans: &[&str] = &["unrestricted", "ham-iaru-r1", "ham-iaru-r2", "ham-iaru-r3"];
    let bp_sel = bandplans
        .iter()
        .copied()
        .find(|&b| b == c.bandplan_mode.as_str());
    let mode_sel = MODES.iter().copied().find(|&m| m == c.mode.as_str());

    Column::new()
        .spacing(6)
        .push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(neutral_btn(
                    eff,
                    "Fetch",
                    Message::FetchConfig,
                    "Re-read the daemon configuration into the editor",
                ))
                .push(accent_btn(
                    eff,
                    "Apply",
                    Message::ApplyConfig,
                    ColorRole::Locked,
                    "Apply the edited configuration to the daemon (SetConfig)",
                ))
                .push(Space::with_width(Length::Fill))
                .push(toggle_btn(
                    eff,
                    "Repeater",
                    snap.repeater_enabled,
                    Message::ToggleRepeater,
                    "Enable / disable the cross-band repeater",
                )),
        )
        .push(info_row(
            eff,
            "Callsign",
            if c.callsign.is_empty() {
                "—"
            } else {
                &c.callsign
            },
            ColorRole::RxValue,
        ))
        .push(info_row(
            eff,
            "Grid",
            if c.grid_square.is_empty() {
                "—"
            } else {
                &c.grid_square
            },
            ColorRole::RxValue,
        ))
        .push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    Text::new("Mode")
                        .size(13)
                        .width(Length::Fixed(78.0))
                        .color(role(eff, ColorRole::Inactive)),
                )
                .push(tip(
                    pick_list(MODES, mode_sel, |m: &str| Message::CfgMode(m.to_string()))
                        .text_size(13),
                    "Configured default modem mode (applied on Apply)",
                    eff,
                )),
        )
        .push(slider_row(
            eff,
            &format!("TX atten {:.1} dB", c.tx_attenuation_db),
            -30.0..=0.0,
            c.tx_attenuation_db,
            Message::CfgAtten,
            "Configured TX attenuation (applied on Apply)",
        ))
        .push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(
                    Text::new("Bandplan")
                        .size(13)
                        .width(Length::Fixed(78.0))
                        .color(role(eff, ColorRole::Inactive)),
                )
                .push(tip(
                    pick_list(bandplans, bp_sel, |b: &str| {
                        Message::CfgBandplan(b.to_string())
                    })
                    .text_size(13),
                    "Bandplan guardrail (unrestricted / IARU R1–R3)",
                    eff,
                )),
        )
        .into()
}

/// A text-only "link" button (no background) in a role colour.
fn link_btn<'a>(
    eff: EffectiveTheme,
    label: &str,
    msg: Message,
    r: ColorRole,
    tip_text: &'static str,
) -> Element<'a, Message> {
    let col = role(eff, r);
    let btn = Button::new(Text::new(label.to_string()).size(12).color(col))
        .padding([2, 4])
        .on_press(msg)
        .style(move |_t: &Theme, _s: button::Status| button::Style {
            background: None,
            text_color: col,
            ..button::Style::default()
        });
    tip(btn, tip_text, eff)
}

/// UTC time-of-day `HH:MMZ` from a Unix timestamp.
fn hhmm(ts: u64) -> String {
    let h = (ts / 3600) % 24;
    let m = (ts / 60) % 60;
    format!("{h:02}:{m:02}Z")
}

fn log_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let mut col = Column::new().spacing(2);
    for line in &snap.log {
        col = col.push(
            Text::new(line.clone())
                .size(11)
                .color(role(eff, ColorRole::Inactive)),
        );
    }
    scrollable(col).height(Length::Fill).into()
}

/// Statistics tab: count of successfully transferred frames per ladder step, this session.
///
/// Buckets are filled by `PanelState::record_frame` on each `FrameReceived` / `FrameTransmitted` event
/// (keyed on the ladder step current at that moment) and cleared on `SessionStarted`. Rows with no
/// frames are hidden; step `—` is the pre-first-`RateChange` bucket.
fn stats_widget(snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let cell = move |s: String, w: f32, r: ColorRole| -> Element<'static, Message> {
        Text::new(s)
            .size(12)
            .width(Length::Fixed(w))
            .color(role(eff, r))
            .into()
    };
    let stat_row = move |step: String,
                         rx: String,
                         tx: String,
                         total: String,
                         r: ColorRole|
          -> Row<'static, Message> {
        Row::new()
            .spacing(8)
            .push(cell(step, 90.0, r))
            .push(cell(rx, 64.0, r))
            .push(cell(tx, 64.0, r))
            .push(cell(total, 64.0, r))
    };

    let mut col = Column::new()
        .spacing(3)
        .push(col_title(eff, "Frames per ladder step (this session)"))
        .push(stat_row(
            "Step".into(),
            "RX".into(),
            "TX".into(),
            "Total".into(),
            ColorRole::Signal,
        ));

    let (mut total_rx, mut total_tx) = (0u32, 0u32);
    let mut any = false;
    let buckets = snap.rx_frames_by_level.len();
    for i in 0..buckets {
        let rx = snap.rx_frames_by_level.get(i).copied().unwrap_or(0);
        let tx = snap.tx_frames_by_level.get(i).copied().unwrap_or(0);
        total_rx = total_rx.saturating_add(rx);
        total_tx = total_tx.saturating_add(tx);
        if rx == 0 && tx == 0 {
            continue;
        }
        any = true;
        let step = if i == 0 {
            "—".to_string()
        } else {
            format!("SL{i}")
        };
        col = col.push(stat_row(
            step,
            rx.to_string(),
            tx.to_string(),
            rx.saturating_add(tx).to_string(),
            ColorRole::RxValue,
        ));
    }

    col = if any {
        col.push(stat_row(
            "Total".into(),
            total_rx.to_string(),
            total_tx.to_string(),
            total_rx.saturating_add(total_tx).to_string(),
            ColorRole::Locked,
        ))
    } else {
        col.push(
            Text::new("No frames transferred yet this session.")
                .size(12)
                .color(role(eff, ColorRole::Inactive)),
        )
    };

    scrollable(col).height(Length::Fill).into()
}

/// Format a compression ratio (compressed/raw, e.g. 0.20) as an intuitive reduction factor ("5.0:1").
///
/// The daemon reports compressed/raw, so a good compression reads as a small fraction (0.20 = a fifth of
/// the size). Displaying that fraction with a "×" made "0.20×" look like *expansion*; showing the
/// reciprocal as an "N:1" ratio matches the gross-vs-effective factor the operator sees. Anything ≥ 1
/// (no gain, or the not-yet-wired 1.0 default) reads "1.0:1"; a non-positive/NaN value shows "—".
fn format_compression(ratio: f32) -> String {
    if !ratio.is_finite() || ratio <= 0.0 {
        return "—".to_string();
    }
    let factor = if ratio < 1.0 { 1.0 / ratio } else { 1.0 };
    format!("{factor:.1}:1")
}

// --- small widget helpers ---------------------------------------------------

fn info_row(
    eff: EffectiveTheme,
    label: &str,
    value: &str,
    value_role: ColorRole,
) -> Element<'static, Message> {
    Row::new()
        .spacing(8)
        .push(
            Text::new(label.to_string())
                .size(13)
                .width(Length::Fixed(78.0))
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(
            Text::new(value.to_string())
                .size(13)
                .color(role(eff, value_role)),
        )
        .into()
}

fn bar_row(eff: EffectiveTheme, label: &str, frac: f32, r: ColorRole) -> Element<'static, Message> {
    let fill = role(eff, r);
    let track = shade(eff, Shade::Track);
    // A fixed-width track with a proportional fill via two flex portions.
    let filled = (frac.clamp(0.0, 1.0) * 1000.0) as u16;
    let rest = 1000 - filled;
    let track_bar = Container::new(
        Row::new()
            .push(
                Container::new(Space::new(Length::Fill, Length::Fixed(8.0)))
                    .width(Length::FillPortion(filled.max(1)))
                    .style(move |_t: &Theme| container::Style {
                        background: Some(Background::Color(fill)),
                        border: Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..container::Style::default()
                    }),
            )
            .push(Space::with_width(Length::FillPortion(rest.max(1)))),
    )
    .width(Length::Fixed(130.0))
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(track)),
        border: Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..container::Style::default()
    });
    Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(
            Text::new(label.to_string())
                .size(13)
                .width(Length::Fixed(78.0))
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(track_bar)
        .into()
}

fn slider_row<'a>(
    eff: EffectiveTheme,
    label: &str,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    on_change: impl Fn(f32) -> Message + 'a,
    tip_text: &'static str,
) -> Element<'a, Message> {
    Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(
            Text::new(label.to_string())
                .size(12)
                .width(Length::Fixed(120.0))
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(tip(
            slider(range, value, on_change).width(Length::Fixed(160.0)),
            tip_text,
            eff,
        ))
        .into()
}

fn chip(eff: EffectiveTheme, label: &str, bg: Color, fg: Color) -> Element<'static, Message> {
    Container::new(Text::new(label.to_string()).size(11).color(fg))
        .padding([3, 6])
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: shade(eff, Shade::Edge),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn state_chip(
    eff: EffectiveTheme,
    label: &str,
    active: bool,
    accent: ColorRole,
) -> Element<'static, Message> {
    let (bg, fg) = if active {
        (role(eff, accent), shade(eff, Shade::Bg))
    } else {
        (shade(eff, Shade::Track), role(eff, ColorRole::Inactive))
    };
    chip(eff, label, bg, fg)
}

fn accent_btn<'a>(
    eff: EffectiveTheme,
    label: &str,
    msg: Message,
    accent: ColorRole,
    tip_text: &'static str,
) -> Element<'a, Message> {
    let bg = role(eff, accent);
    let text = shade(eff, Shade::Bg);
    let edge = shade(eff, Shade::Edge);
    let btn = Button::new(Text::new(label.to_string()).size(13).color(text))
        .padding([6, 11])
        .on_press(msg)
        .style(move |_t: &Theme, status: button::Status| {
            let b = match status {
                button::Status::Hovered | button::Status::Pressed => lerp(bg, Color::WHITE, 0.12),
                _ => bg,
            };
            button::Style {
                background: Some(Background::Color(b)),
                text_color: text,
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..button::Style::default()
            }
        });
    tip(btn, tip_text, eff)
}

fn neutral_btn<'a>(
    eff: EffectiveTheme,
    label: &str,
    msg: Message,
    tip_text: &'static str,
) -> Element<'a, Message> {
    let rest = shade(eff, Shade::Control);
    let hover = shade(eff, Shade::ControlHover);
    let text = role(eff, ColorRole::RxValue);
    let edge = shade(eff, Shade::Edge);
    let btn = Button::new(Text::new(label.to_string()).size(13).color(text))
        .padding([6, 11])
        .on_press(msg)
        .style(move |_t: &Theme, status: button::Status| {
            let b = match status {
                button::Status::Hovered | button::Status::Pressed => hover,
                _ => rest,
            };
            button::Style {
                background: Some(Background::Color(b)),
                text_color: text,
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: 5.0.into(),
                },
                ..button::Style::default()
            }
        });
    tip(btn, tip_text, eff)
}

fn toggle_btn<'a>(
    eff: EffectiveTheme,
    label: &str,
    on: bool,
    msg: Message,
    tip_text: &'static str,
) -> Element<'a, Message> {
    let accent = if on {
        ColorRole::Locked
    } else {
        ColorRole::Inactive
    };
    accent_btn(
        eff,
        &format!("{label}: {}", if on { "ON" } else { "off" }),
        msg,
        accent,
        tip_text,
    )
}

fn heat_role(pct: f32) -> ColorRole {
    if pct > 85.0 {
        ColorRole::TxActive
    } else if pct > 60.0 {
        ColorRole::Caution
    } else {
        ColorRole::Locked
    }
}

fn fmt_rig(r: &RigSnapshot) -> String {
    let mut s = format!("{:.3} MHz {}", r.freq_hz as f64 / 1e6, r.mode);
    if let Some(p) = r.power_w {
        s.push_str(&format!(" {p:.0}W"));
    }
    if let Some(swr) = r.swr {
        s.push_str(&format!(" SWR {swr:.1}"));
    }
    if let Some(alc) = r.alc {
        s.push_str(&format!(" ALC {alc:.2}"));
    }
    s
}

// --- canvas programs --------------------------------------------------------

struct SpectrumTrace {
    trace: Vec<f32>,
    bg: Color,
    line: Color,
    grid: Color,
}

impl canvas::Program<Message> for SpectrumTrace {
    type State = ();
    fn draw(
        &self,
        _s: &(),
        renderer: &Renderer,
        _t: &Theme,
        bounds: Rectangle,
        _c: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), self.bg);
        for d in 1..4 {
            let y = d as f32 / 4.0 * h;
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(w, y)),
                Stroke::default().with_width(1.0).with_color(self.grid),
            );
        }
        if self.trace.len() > 1 {
            let n = self.trace.len();
            let trace = Path::new(|b| {
                for (i, &db) in self.trace.iter().enumerate() {
                    let x = i as f32 / (n - 1) as f32 * w;
                    let y = dbm_to_y(db, h);
                    if i == 0 {
                        b.move_to(Point::new(x, y));
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &trace,
                Stroke::default().with_width(1.5).with_color(self.line),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// ECC-rate trend line plot (newest sample at the right), 0–10 % full-scale.
struct EccTrend {
    hist: Vec<f32>,
    bg: Color,
    line: Color,
    grid: Color,
}

impl canvas::Program<Message> for EccTrend {
    type State = ();
    fn draw(
        &self,
        _s: &(),
        renderer: &Renderer,
        _t: &Theme,
        bounds: Rectangle,
        _c: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), self.bg);
        let mid = Path::line(Point::new(0.0, h / 2.0), Point::new(w, h / 2.0));
        frame.stroke(
            &mid,
            Stroke::default().with_width(1.0).with_color(self.grid),
        );
        // `hist` is newest-first; draw oldest → newest left to right, 0..0.10 mapped to full height.
        let n = self.hist.len();
        if n > 1 {
            let full = 0.10f32;
            let path = Path::new(|b| {
                for (i, &v) in self.hist.iter().rev().enumerate() {
                    let x = i as f32 / (n - 1) as f32 * w;
                    let y = (1.0 - (v / full).clamp(0.0, 1.0)) * h;
                    if i == 0 {
                        b.move_to(Point::new(x, y));
                    } else {
                        b.line_to(Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &path,
                Stroke::default().with_width(1.5).with_color(self.line),
            );
        }
        vec![frame.into_geometry()]
    }
}

struct Waterfall {
    rows: Vec<Vec<f32>>,
    low: Color,
    mid: Color,
    high: Color,
}

impl canvas::Program<Message> for Waterfall {
    type State = ();
    fn draw(
        &self,
        _s: &(),
        renderer: &Renderer,
        _t: &Theme,
        bounds: Rectangle,
        _c: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), self.low);
        let rows = self.rows.len();
        if rows == 0 {
            return vec![frame.into_geometry()];
        }
        let row_h = (h / rows as f32).max(1.0);
        let min = TOP_DB - RANGE_DB;
        for (r, row) in self.rows.iter().enumerate() {
            let cols = row.len().max(1);
            let col_w = (w / cols as f32).max(1.0);
            let y = r as f32 * row_h;
            for (c, &db) in row.iter().enumerate() {
                let t = ((db - min) / RANGE_DB).clamp(0.0, 1.0);
                let color = if t < 0.5 {
                    lerp(self.low, self.mid, t * 2.0)
                } else {
                    lerp(self.mid, self.high, (t - 0.5) * 2.0)
                };
                frame.fill_rectangle(
                    Point::new(c as f32 * col_w, y),
                    Size::new(col_w + 1.0, row_h + 1.0),
                    color,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::format_compression;

    #[test]
    fn compression_shows_reduction_factor() {
        // compressed/raw 0.20 → a 5:1 reduction (the operator's "factor 5").
        assert_eq!(format_compression(0.20), "5.0:1");
        assert_eq!(format_compression(0.50), "2.0:1");
        assert_eq!(format_compression(0.25), "4.0:1");
    }

    #[test]
    fn compression_no_gain_reads_one_to_one() {
        // Not-yet-wired default (1.0) and incompressible (>1) both read as no gain, never expansion.
        assert_eq!(format_compression(1.0), "1.0:1");
        assert_eq!(format_compression(1.2), "1.0:1");
    }

    #[test]
    fn compression_guards_bad_values() {
        assert_eq!(format_compression(0.0), "—");
        assert_eq!(format_compression(-0.5), "—");
        assert_eq!(format_compression(f32::NAN), "—");
    }
}
