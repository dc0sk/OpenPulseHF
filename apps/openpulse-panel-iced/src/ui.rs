//! The panel view: a fixed vertical stack — spectrum → waterfall → ladder → additional info →
//! controls (REQ-UX-04) — plus the iced colour mapping for the active theme.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, container, Button, Column, Container, Row, Text};
use iced::{
    mouse, Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size,
    Theme,
};

use crate::app::{App, Message, LADDER_RUNGS};
use crate::theme::{role_rgb, shade_rgb, ColorRole, EffectiveTheme, Shade};

// --- spectrum window (dBm) ---
const TOP_DBM: f32 = -20.0;
const RANGE_DB: f32 = 100.0;

/// Surface shade → iced colour for the active theme.
pub fn shade(eff: EffectiveTheme, s: Shade) -> Color {
    let (r, g, b) = shade_rgb(eff, s);
    Color::from_rgb8(r, g, b)
}

/// Semantic role → iced colour for the active theme.
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

fn dbm_to_y(dbm: f32, h: f32) -> f32 {
    ((TOP_DBM - dbm) / RANGE_DB * h).clamp(0.0, h)
}

/// The full stacked view.
pub fn view(app: &App) -> Element<'_, Message> {
    let eff = app.effective_theme();

    let stack = Column::new()
        .spacing(8)
        .padding(10)
        .push(panel(eff, "Spectrum", spectrum_widget(app, eff)))
        .push(panel(eff, "Waterfall", waterfall_widget(app, eff)))
        .push(panel(eff, "Ladder", ladder_widget(app, eff)))
        .push(panel(eff, "Info", info_widget(app, eff)))
        .push(panel(eff, "Controls", controls_widget(app, eff)));

    Container::new(stack)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(shade(eff, Shade::Bg))),
            ..container::Style::default()
        })
        .into()
}

/// A titled grouping panel (one stack section).
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

fn spectrum_widget(app: &App, eff: EffectiveTheme) -> Element<'_, Message> {
    let canvas = Canvas::new(SpectrumTrace {
        trace: &app.spectrum,
        bg: shade(eff, Shade::Track),
        line: role(eff, ColorRole::Signal),
        grid: shade(eff, Shade::Edge),
    })
    .width(Length::Fill)
    .height(Length::Fixed(150.0));
    Element::from(canvas)
}

fn waterfall_widget(app: &App, eff: EffectiveTheme) -> Element<'_, Message> {
    let canvas = Canvas::new(Waterfall {
        rows: &app.waterfall,
        low: shade(eff, Shade::Track),
        mid: role(eff, ColorRole::Signal),
        high: role(eff, ColorRole::Caution),
    })
    .width(Length::Fill)
    .height(Length::Fixed(130.0));
    Element::from(canvas)
}

fn ladder_widget(app: &App, eff: EffectiveTheme) -> Element<'_, Message> {
    let mut row = Row::new().spacing(4).align_y(Alignment::Center);
    for sl in 1..=LADDER_RUNGS {
        let (bg, fg) = if sl == app.current_sl {
            (role(eff, ColorRole::Locked), shade(eff, Shade::Bg))
        } else if sl < app.current_sl {
            (shade(eff, Shade::Control), role(eff, ColorRole::Signal))
        } else {
            (shade(eff, Shade::Track), role(eff, ColorRole::Inactive))
        };
        let chip = Container::new(Text::new(format!("SL{sl}")).size(12).color(fg))
            .padding([4, 8])
            .style(move |_t: &Theme| container::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    color: shade(eff, Shade::Edge),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..container::Style::default()
            });
        row = row.push(chip);
    }
    row.into()
}

fn info_widget(app: &App, eff: EffectiveTheme) -> Element<'_, Message> {
    let state_role = if app.connected {
        ColorRole::Locked
    } else {
        ColorRole::Inactive
    };
    let tx_role = if app.tx {
        ColorRole::TxActive
    } else {
        ColorRole::Inactive
    };
    Column::new()
        .spacing(4)
        .push(info_row(eff, "State", app.state, state_role))
        .push(info_row(eff, "Mode", app.mode, ColorRole::Signal))
        .push(info_row(
            eff,
            "SNR",
            &format!("{:.1} dB", app.snr_db),
            ColorRole::RxValue,
        ))
        .push(info_row(
            eff,
            "Rung",
            &format!("SL{}", app.current_sl),
            ColorRole::Locked,
        ))
        .push(info_row(
            eff,
            "TX",
            if app.tx { "ON" } else { "off" },
            tx_role,
        ))
        .into()
}

fn info_row<'a>(
    eff: EffectiveTheme,
    label: &'a str,
    value: &str,
    value_role: ColorRole,
) -> Element<'a, Message> {
    Row::new()
        .spacing(8)
        .push(
            Text::new(label)
                .size(13)
                .width(Length::Fixed(70.0))
                .color(role(eff, ColorRole::Inactive)),
        )
        .push(
            Text::new(value.to_string())
                .size(13)
                .color(role(eff, value_role)),
        )
        .into()
}

fn controls_widget(app: &App, eff: EffectiveTheme) -> Element<'_, Message> {
    let (conn_label, conn_role) = if app.connected {
        ("Disconnect", ColorRole::TxActive)
    } else {
        ("Connect", ColorRole::Locked)
    };
    let ptt = themed_button(
        eff,
        if app.tx { "PTT ●" } else { "PTT" },
        Message::ToggleTx,
        if app.tx {
            ColorRole::TxActive
        } else {
            ColorRole::Inactive
        },
    );
    // The theme toggle: a neutral utility control; the label shows the current mode and a click
    // cycles Dark→Light→Contrast→System.
    let theme_btn = neutral_button(
        eff,
        &format!("Theme: {}", app.theme_mode.label()),
        Message::ToggleTheme,
    );
    Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(themed_button(
            eff,
            conn_label,
            Message::ToggleConnect,
            conn_role,
        ))
        .push(ptt)
        .push(iced::widget::horizontal_space())
        .push(theme_btn)
        .into()
}

fn themed_button<'a>(
    eff: EffectiveTheme,
    label: &str,
    msg: Message,
    accent: ColorRole,
) -> Button<'a, Message> {
    let accent = role(eff, accent);
    let text = shade(eff, Shade::Bg);
    let edge = shade(eff, Shade::Edge);
    Button::new(Text::new(label.to_string()).size(13).color(text))
        .padding([6, 12])
        .on_press(msg)
        .style(move |_t: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    lerp(accent, Color::WHITE, 0.12)
                }
                _ => accent,
            };
            button::Style {
                background: Some(Background::Color(bg)),
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

/// A neutral (non-accent) utility button: control-surface background that lifts to `ControlHover`
/// under the pointer, with readout-coloured text — the visual grammar of a rest-state control.
fn neutral_button<'a>(eff: EffectiveTheme, label: &str, msg: Message) -> Button<'a, Message> {
    let rest = shade(eff, Shade::Control);
    let hover = shade(eff, Shade::ControlHover);
    let text = role(eff, ColorRole::RxValue);
    let edge = shade(eff, Shade::Edge);
    Button::new(Text::new(label.to_string()).size(13).color(text))
        .padding([6, 12])
        .on_press(msg)
        .style(move |_t: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => hover,
                _ => rest,
            };
            button::Style {
                background: Some(Background::Color(bg)),
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

// --- canvas programs --------------------------------------------------------

struct SpectrumTrace<'a> {
    trace: &'a [f32],
    bg: Color,
    line: Color,
    grid: Color,
}

impl<Message> canvas::Program<Message> for SpectrumTrace<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), self.bg);

        // Horizontal grid lines every ~25 dB.
        let divs = (RANGE_DB / 25.0) as usize;
        for d in 1..divs {
            let y = d as f32 / divs as f32 * h;
            let line = Path::line(Point::new(0.0, y), Point::new(w, y));
            frame.stroke(
                &line,
                Stroke::default().with_width(1.0).with_color(self.grid),
            );
        }

        if self.trace.len() > 1 {
            let n = self.trace.len();
            let trace = Path::new(|b| {
                for (i, &dbm) in self.trace.iter().enumerate() {
                    let x = i as f32 / (n - 1) as f32 * w;
                    let y = dbm_to_y(dbm, h);
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

struct Waterfall<'a> {
    rows: &'a [Vec<f32>],
    low: Color,
    mid: Color,
    high: Color,
}

impl<Message> canvas::Program<Message> for Waterfall<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), self.low);

        let rows = self.rows.len();
        if rows == 0 {
            return vec![frame.into_geometry()];
        }
        let row_h = (h / rows as f32).max(1.0);
        let min_db = TOP_DBM - RANGE_DB;
        for (r, row) in self.rows.iter().enumerate() {
            let cols = row.len().max(1);
            let col_w = (w / cols as f32).max(1.0);
            let y = r as f32 * row_h;
            for (c, &dbm) in row.iter().enumerate() {
                let t = ((dbm - min_db) / RANGE_DB).clamp(0.0, 1.0);
                // low → mid → high across the dynamic range.
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
