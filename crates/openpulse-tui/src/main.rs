//! `openpulse-tui` — live TUI dashboard for the OpenPulse modem engine.

use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tracing::Level;

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

#[cfg(feature = "cpal-backend")]
use openpulse_audio::CpalBackend;

mod app;
mod events;
mod ui;

#[derive(clap::Parser)]
#[command(name = "openpulse-tui", about = "OpenPulse TUI dashboard")]
struct Cli {
    /// Modulation mode to drive the receive loop.
    #[arg(short, long, default_value = "BPSK100")]
    mode: String,

    /// Audio backend: loopback | default | cpal.
    #[arg(long, default_value = "loopback")]
    backend: String,

    /// Verbosity level.
    #[arg(long, default_value = "warn")]
    log: String,
}

fn main() -> Result<()> {
    use clap::Parser as _;
    let cli = Cli::parse();

    let level: Level = cli.log.parse().unwrap_or(Level::WARN);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();

    let audio: Box<dyn openpulse_core::audio::AudioBackend> = match cli.backend.as_str() {
        "loopback" => Box::new(LoopbackBackend::new()),
        #[cfg(feature = "cpal-backend")]
        "default" | "cpal" => Box::new(CpalBackend::new()),
        #[cfg(not(feature = "cpal-backend"))]
        "default" => {
            eprintln!(
                "note: cpal backend not compiled in (enable --features cpal-backend); falling back to loopback"
            );
            Box::new(LoopbackBackend::new())
        }
        name => anyhow::bail!("unknown backend '{name}'"),
    };

    let mut engine = ModemEngine::new(audio);
    engine.register_plugin(Box::new(BpskPlugin::new()))?;
    engine.register_plugin(Box::new(Fsk4Plugin::new()))?;
    engine.register_plugin(Box::new(Psk8Plugin::new()))?;
    engine.register_plugin(Box::new(QpskPlugin::new()))?;

    let rx = engine.subscribe();
    let worker = events::spawn_worker(engine, cli.mode.clone(), rx);

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui(&mut terminal, worker);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    worker: std::sync::mpsc::Receiver<events::WorkerMsg>,
) -> Result<()> {
    let mut app = app::App::default();
    let tick = Duration::from_millis(100);

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('p'), _) => app.paused = !app.paused,
                    (KeyCode::Up, _) => {
                        app.scroll_offset = app.scroll_offset.saturating_sub(1);
                    }
                    (KeyCode::Down, _) => {
                        app.scroll_offset = app.scroll_offset.saturating_add(1);
                    }
                    _ => {}
                }
            }
        }

        match events::drain_worker(&mut app, &worker) {
            Ok(()) => {}
            Err(e) => {
                // Surface fatal worker error before exiting.
                app.fatal_error = Some(e);
                terminal.draw(|f| ui::render(f, &app))?;
                std::thread::sleep(Duration::from_secs(3));
                break;
            }
        }
    }

    Ok(())
}
