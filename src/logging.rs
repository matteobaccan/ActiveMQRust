// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Logging setup: one line per event with a local timestamp, to stdout or to a file.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Mutex;

use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

fn level(name: &str) -> LevelFilter {
    match name {
        "error" => LevelFilter::ERROR,
        "warn" => LevelFilter::WARN,
        "debug" => LevelFilter::DEBUG,
        "trace" => LevelFilter::TRACE,
        _ => LevelFilter::INFO,
    }
}

/// Logs to stdout (console mode).
pub fn init_stdout(level_name: &str) {
    let _ = tracing_subscriber::fmt()
        .with_timer(LocalTime)
        .with_target(false)
        .with_ansi(false)
        .with_max_level(level(level_name))
        .try_init();
}

/// Logs to a file, appending (service mode).
pub fn init_file(path: &Path, level_name: &str) -> std::io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let _ = tracing_subscriber::fmt()
        .with_timer(LocalTime)
        .with_target(false)
        .with_ansi(false)
        .with_max_level(level(level_name))
        .with_writer(Mutex::new(file))
        .try_init();
    Ok(())
}
