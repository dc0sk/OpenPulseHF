//! The panel view (REQ-UX-04), rendered from the live `PanelState` with the active theme's palette:
//! a controls band on top, then spectrum → waterfall → ladder, an Additional-info | Daemon-config
//! row, and a tabbed Messages / Event-log panel.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{
    button, container, pick_list, scrollable, slider, text_input, Button, Column, Container, Row,
    Space, Text,
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

// Spectrum window (dBFS): 0 dB at the top down to −120 dB.
const TOP_DB: f32 = 0.0;
const RANGE_DB: f32 = 120.0;

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
        }
    };

    // Additional info + Daemon config side by side (config took the controls' old slot).
    let info_config = Row::new()
        .spacing(8)
        .width(Length::Fill)
        .push(
            Container::new(panel(eff, "Additional info", info_widget(&snap, eff)))
                .width(Length::FillPortion(1)),
        )
        .push(
            Container::new(panel(eff, "Daemon config", config_widget(app, &snap, eff)))
                .width(Length::FillPortion(1)),
        );

    let stack = Column::new()
        .spacing(8)
        .padding(10)
        // Controls band across the top.
        .push(panel(eff, "Controls", controls_widget(app, &snap, eff)))
        .push(panel(eff, "Spectrum", spectrum_widget(&snap, eff)))
        .push(panel(eff, "Waterfall", waterfall_widget(&snap, eff)))
        .push(panel(eff, "Ladder", ladder_widget(&snap, eff)))
        .push(info_config)
        // Messages / Event log as one tabbed panel.
        .push(tabbed_messages_log(app, &snap, eff));

    let scroll = scrollable(stack).height(Length::Fill);

    Container::new(scroll)
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
    .height(Length::Fixed(140.0))
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
    .height(Length::Fixed(120.0))
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
    let mut col = Column::new().spacing(4);
    col = col
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
            &format!("{:.2}×", snap.compress_ratio),
            ColorRole::RxValue,
        ))
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
        col = col.push(info_row(
            eff,
            "S-meter",
            &format!("{dbm} dBm"),
            ColorRole::Signal,
        ));
    }
    // Resources
    col = col
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
        col = col.push(bar_row(eff, "GPU", g / 100.0, heat_role(g)));
    }
    col = col.push(info_row(
        eff,
        "Decode",
        &format!("{:.1} ms", snap.decode_latency_ms),
        ColorRole::RxValue,
    ));

    if let Some(r) = &snap.rig_a {
        col = col.push(info_row(eff, "Rig A", &fmt_rig(r), ColorRole::RxValue));
    }
    if let Some(r) = &snap.rig_b {
        col = col.push(info_row(eff, "Rig B", &fmt_rig(r), ColorRole::RxValue));
    }
    // ECC-rate trend (rolling ~2 min).
    col = col
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
        );
    col.into()
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
    let conn = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(
            pick_list(transports, Some(tsel), |s: &str| {
                Message::SelectTransport(s == "WS")
            })
            .text_size(13),
        )
        .push(
            text_input("host:port", &app.addr)
                .on_input(Message::AddrChanged)
                .size(13)
                .width(Length::Fixed(150.0)),
        )
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
        ))
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
        ));

    // Mode + frequency.
    let sel = MODES.iter().copied().find(|&m| m == app.mode_sel.as_str());
    let mode_freq = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(pick_list(MODES, sel, |m: &str| Message::ModeSelected(m.to_string())).text_size(13))
        .push(
            text_input("kHz", &app.freq_khz)
                .on_input(Message::FreqChanged)
                .size(13)
                .width(Length::Fixed(90.0)),
        )
        .push(neutral_btn(eff, "Tune", Message::TuneFreq));

    // Feature toggles (Repeater lives in the Config panel now).
    let toggles = Row::new()
        .spacing(6)
        .push(toggle_btn(
            eff,
            "CE-SSB",
            app.cessb_on,
            Message::ToggleCessb,
        ))
        .push(toggle_btn(eff, "Notch", app.notch_on, Message::ToggleNotch))
        .push(toggle_btn(eff, "AGC", app.agc_on, Message::ToggleAgc))
        .push(toggle_btn(
            eff,
            "Logbook",
            app.logbook_on,
            Message::ToggleLogbook,
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
        ))
        .push(slider_row(
            eff,
            &format!("Squelch {:.3}", app.squelch),
            0.0..=0.2,
            app.squelch,
            Message::SquelchChanged,
        ));

    // RF peer.
    let peer = if snap.rf_connected {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                Text::new(format!(
                    "RF peer: {}",
                    snap.rf_peer.clone().unwrap_or_default()
                ))
                .size(13)
                .color(role(eff, ColorRole::Locked)),
            )
            .push(accent_btn(
                eff,
                "Disconnect RF",
                Message::DisconnectPeer,
                ColorRole::TxActive,
            ))
    } else {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                text_input("CALLSIGN", &app.peer_call)
                    .on_input(Message::PeerCallChanged)
                    .size(13)
                    .width(Length::Fixed(120.0)),
            )
            .push(accent_btn(
                eff,
                "Connect RF",
                Message::ConnectPeer,
                ColorRole::Locked,
            ))
    };

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
            ))
            .push(accent_btn(
                eff,
                "Stop",
                Message::StopOta,
                ColorRole::TxActive,
            ))
    } else {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                text_input("profile", &app.ota_profile)
                    .on_input(Message::OtaProfileChanged)
                    .size(13)
                    .width(Length::Fixed(110.0)),
            )
            .push(accent_btn(
                eff,
                "Start OTA",
                Message::StartOta,
                ColorRole::Locked,
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
        ));

    // Line 3: sliders side by side, then RF peer + OTA.
    let line3 = Row::new()
        .spacing(16)
        .align_y(Alignment::Center)
        .push(sliders)
        .push(Space::with_width(Length::Fill))
        .push(peer)
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
                ))
                .push(accent_btn(
                    eff,
                    "Reject",
                    Message::RejectQsy,
                    ColorRole::TxActive,
                )),
        );
    }
    col.into()
}

/// Messages and Event log as one tabbed panel.
fn tabbed_messages_log(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let tab_btn = |label: &str, tab: Tab| -> Element<'static, Message> {
        let active = app.active_tab == tab;
        let (bg, fg) = if active {
            (role(eff, ColorRole::Signal), shade(eff, Shade::Bg))
        } else {
            (shade(eff, Shade::Control), role(eff, ColorRole::Inactive))
        };
        Button::new(Text::new(label.to_string()).size(12).color(fg))
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
            })
            .into()
    };
    let header = Row::new()
        .spacing(4)
        .push(tab_btn("Messages", Tab::Messages))
        .push(tab_btn("Event log", Tab::Log));
    let content = match app.active_tab {
        Tab::Messages => messages_widget(app, snap, eff),
        Tab::Log => log_widget(snap, eff),
    };
    let body = Column::new().spacing(8).push(header).push(content);
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

fn messages_widget(app: &App, snap: &Snap, eff: EffectiveTheme) -> Element<'static, Message> {
    let header = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(
            Text::new(format!("Inbox ({})", snap.inbox.len()))
                .size(13)
                .color(role(eff, ColorRole::RxValue)),
        )
        .push(neutral_btn(eff, "Refresh", Message::RefreshInbox));

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
            .push(link_btn(eff, &label, Message::OpenMsg(m.id), accent))
            .push(Space::with_width(Length::Fill))
            .push(link_btn(
                eff,
                "✕",
                Message::DeleteMsg(m.id),
                ColorRole::TxActive,
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
                .push(
                    text_input("CALLSIGN", &app.msg_to)
                        .on_input(Message::MsgTo)
                        .size(13)
                        .width(Length::Fixed(120.0)),
                )
                .push(
                    text_input("subject", &app.msg_subject)
                        .on_input(Message::MsgSubject)
                        .size(13)
                        .width(Length::Fill),
                ),
        )
        .push(
            text_input("Message body…", &app.msg_body)
                .on_input(Message::MsgBody)
                .size(13),
        )
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
                .push(neutral_btn(eff, "Fetch", Message::FetchConfig))
                .push(accent_btn(
                    eff,
                    "Apply",
                    Message::ApplyConfig,
                    ColorRole::Locked,
                ))
                .push(Space::with_width(Length::Fill))
                .push(toggle_btn(
                    eff,
                    "Repeater",
                    snap.repeater_enabled,
                    Message::ToggleRepeater,
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
                .push(
                    pick_list(MODES, mode_sel, |m: &str| Message::CfgMode(m.to_string()))
                        .text_size(13),
                ),
        )
        .push(slider_row(
            eff,
            &format!("TX atten {:.1} dB", c.tx_attenuation_db),
            -30.0..=0.0,
            c.tx_attenuation_db,
            Message::CfgAtten,
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
                .push(
                    pick_list(bandplans, bp_sel, |b: &str| {
                        Message::CfgBandplan(b.to_string())
                    })
                    .text_size(13),
                ),
        )
        .push(toggle_btn(
            eff,
            "QSY",
            c.qsy_enabled,
            Message::CfgQsy(!c.qsy_enabled),
        ))
        .push(toggle_btn(
            eff,
            "Tune on high SWR",
            c.allow_tuner_on_high_swr,
            Message::CfgTuneSwr(!c.allow_tuner_on_high_swr),
        ))
        .into()
}

/// A text-only "link" button (no background) in a role colour.
fn link_btn<'a>(
    eff: EffectiveTheme,
    label: &str,
    msg: Message,
    r: ColorRole,
) -> Button<'a, Message> {
    let col = role(eff, r);
    Button::new(Text::new(label.to_string()).size(12).color(col))
        .padding([2, 4])
        .on_press(msg)
        .style(move |_t: &Theme, _s: button::Status| button::Style {
            background: None,
            text_color: col,
            ..button::Style::default()
        })
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
    scrollable(col).height(Length::Fixed(120.0)).into()
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
        .push(slider(range, value, on_change).width(Length::Fixed(160.0)))
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
) -> Button<'a, Message> {
    let bg = role(eff, accent);
    let text = shade(eff, Shade::Bg);
    let edge = shade(eff, Shade::Edge);
    Button::new(Text::new(label.to_string()).size(13).color(text))
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
        })
}

fn neutral_btn<'a>(eff: EffectiveTheme, label: &str, msg: Message) -> Button<'a, Message> {
    let rest = shade(eff, Shade::Control);
    let hover = shade(eff, Shade::ControlHover);
    let text = role(eff, ColorRole::RxValue);
    let edge = shade(eff, Shade::Edge);
    Button::new(Text::new(label.to_string()).size(13).color(text))
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
        })
}

fn toggle_btn<'a>(eff: EffectiveTheme, label: &str, on: bool, msg: Message) -> Button<'a, Message> {
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
