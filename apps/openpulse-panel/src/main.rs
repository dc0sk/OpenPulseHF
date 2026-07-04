//! OpenPulse operator panel — iced re-implementation (REQ-UX-04).
//!
//! First scaffold increment: renders the fixed vertical stack (spectrum → waterfall → ladder →
//! additional info → controls) with selectable Dark / Light / Contrast / System themes, modeled on
//! the K4remote look&feel. Shows synthetic demo data; the daemon wiring is a later increment.

mod app;
mod connection;
mod state;
mod theme;
mod transport;
mod ui;

use app::App;

pub fn main() -> iced::Result {
    iced::application("OpenPulse Panel", App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .window(iced::window::Settings {
            size: iced::Size::new(1176.0, 967.0),
            min_size: Some(iced::Size::new(1176.0, 967.0)),
            ..Default::default()
        })
        .run_with(App::new)
}
