//! `openpulse-panel` — operator UI for the OpenPulseHF server daemon.

mod app;
mod connection;
mod state;
mod transport;
mod ui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPulse Panel")
            .with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "openpulse-panel",
        options,
        Box::new(|_cc| Ok(Box::new(app::PanelApp::new()))),
    )
}
