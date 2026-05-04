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
            Constraint::Length(5),
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
    let afc = app
        .afc_offset_hz
        .map(|v| format!("{v:+.1} Hz"))
        .unwrap_or_else(|| "—".to_string());
    let mode = app.current_mode.as_deref().unwrap_or("—");
    let sl = app
        .speed_level
        .as_ref()
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| "—".to_string());

    let lines = vec![
        Line::from(vec![
            Span::raw("AFC: "),
            Span::styled(afc, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("Rate: "),
            Span::styled(format!("{sl} {mode}"), Style::default().fg(Color::Yellow)),
        ]),
    ];

    let text = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Meters"));
    f.render_widget(text, area);

    // DCD energy bar drawn below the paragraph — ratatui Gauge widget.
    // We render it on the last line of the area if space permits.
    if area.height >= 4 {
        let gauge_area = ratatui::layout::Rect {
            x: area.x + 1,
            y: area.y + area.height - 2,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let energy_pct = (app.dcd_energy * 100.0).clamp(0.0, 100.0) as u16;
        let dcd_color = if app.dcd_busy {
            Color::Red
        } else {
            Color::Green
        };
        let gauge = Gauge::default()
            .block(Block::default())
            .gauge_style(Style::default().fg(dcd_color))
            .percent(energy_pct)
            .label(format!("DCD {energy_pct}%"));
        f.render_widget(gauge, gauge_area);
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
