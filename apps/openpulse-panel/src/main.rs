//! `openpulse-panel` — operator UI for the OpenPulseHF server daemon.

mod app;
mod connection;
mod state;
mod transport;
mod ui;

#[cfg(not(target_arch = "wasm32"))]
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

/// WASM entry point — called by the browser after the WASM module is loaded.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();

    let canvas = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document")
        .get_element_by_id("the_canvas_id")
        .expect("canvas element 'the_canvas_id' not found")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("element is not a canvas");

    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|_cc| Ok(Box::new(app::PanelApp::new()))),
            )
            .await
            .expect("failed to start eframe");
    });
}
