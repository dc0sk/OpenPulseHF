//! TUI rendering using ratatui.

use openpulse_core::hpx::HpxState;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.size();

    // If a fatal error is set, overlay it across the full terminal.
    if let Some(err) = &app.fatal_error {
        let msg = Paragraph::new(format!("Worker error: {err}\n\nPress q to quit."))
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title("Fatal Error"));
        f.render_widget(msg, area);
        return;
    }

    // Outer split: top panel row + transition log + help bar.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    // Top row: left status | right meters.
    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[0]);

    render_hpx_state(f, app, top_cols[0]);
    render_meters(f, app, top_cols[1]);
    render_transitions(f, app, rows[1]);
    render_help(f, rows[2]);
}

fn render_hpx_state(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let state_str = format!("{:?}", app.hpx_state);
    let color = state_color(app.hpx_state);
    let text = Paragraph::new(Span::styled(
        state_str,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
    .block(Block::default().borders(Borders::ALL).title("HPX State"));
    f.render_widget(text, area);
}

fn render_meters(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let afc_offset = app
        .afc_offset_hz
        .map(|v| format!("{v:+.1} Hz"))
        .unwrap_or_else(|| "—".to_string());
    let afc_corr = app
        .afc_correction_hz
        .map(|v| format!("{v:+.1} Hz"))
        .unwrap_or_else(|| "—".to_string());
    let mode = app.current_mode.as_deref().unwrap_or("—");
    let sl = app
        .speed_level
        .as_ref()
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| "—".to_string());
    let trend = app
        .speed_trend()
        .map(|t| match t {
            crate::app::SpeedTrend::Up => "up",
            crate::app::SpeedTrend::Down => "down",
            crate::app::SpeedTrend::Flat => "flat",
        })
        .unwrap_or("—");

    let block = Block::default().borders(Borders::ALL).title("Meters");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let lines = vec![
        Line::from(vec![
            Span::raw("AFC offset: "),
            Span::styled(afc_offset, Style::default().fg(Color::Cyan)),
            Span::raw("  corr: "),
            Span::styled(afc_corr, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Rate: "),
            Span::styled(format!("{sl} {mode}"), Style::default().fg(Color::Yellow)),
            Span::raw("  trend: "),
            Span::styled(trend, Style::default().fg(Color::LightYellow)),
        ]),
    ];

    let text_height = inner.height.min(2);
    let text_area = ratatui::layout::Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: text_height,
    };
    let text = Paragraph::new(lines);
    f.render_widget(text, text_area);

    if inner.height >= 3 {
        let dcd_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        };
        let energy_pct = (app.dcd_energy * 100.0).clamp(0.0, 100.0) as u16;
        let dcd_color = if app.dcd_busy {
            Color::Red
        } else {
            Color::Green
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(dcd_color))
            .percent(energy_pct)
            .label(format!("DCD {energy_pct}%"));
        f.render_widget(gauge, dcd_area);
    }

    if inner.height >= 4 {
        let fer_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + 3,
            width: inner.width,
            height: 1,
        };
        let fer_pct = app.fer_percent().unwrap_or(0.0).clamp(0.0, 100.0);
        let fer_color = if fer_pct < 5.0 {
            Color::Green
        } else if fer_pct < 15.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(fer_color))
            .percent(fer_pct as u16)
            .label(if app.fer_percent().is_some() {
                format!("FER {fer_pct:.1}%")
            } else {
                "FER n/a".to_string()
            });
        f.render_widget(gauge, fer_area);
    }
}

fn render_transitions(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let total = app.transitions.len();
    let start = app.scroll_offset.min(total.saturating_sub(visible_height));
    let items: Vec<ListItem> = app
        .transitions
        .iter()
        .skip(start)
        .take(visible_height)
        .map(|s| ListItem::new(s.as_str()))
        .collect();

    let title = if app.paused {
        "Recent Transitions (PAUSED)"
    } else {
        "Recent Transitions"
    };
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

fn render_help(f: &mut Frame, area: ratatui::layout::Rect) {
    let help = Paragraph::new("[q] Quit   [p] Pause/Resume   [↑↓] Scroll transitions")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, area);
}

fn state_color(state: HpxState) -> Color {
    match state {
        HpxState::Idle => Color::DarkGray,
        HpxState::Discovery => Color::Blue,
        HpxState::Training => Color::Yellow,
        HpxState::ActiveTransfer => Color::Green,
        HpxState::Recovery => Color::Magenta,
        HpxState::RelayActive => Color::Cyan,
        HpxState::Teardown => Color::Yellow,
        HpxState::Failed => Color::Red,
    }
}
