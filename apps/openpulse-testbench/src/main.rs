use tracing_subscriber::EnvFilter;

mod app;
mod colormap;
mod signal_path;
mod state;
mod ui;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPulse Testbench")
            .with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "openpulse-testbench",
        options,
        Box::new(|_cc| Ok(Box::new(app::TestbenchApp::new()))),
    )
}
