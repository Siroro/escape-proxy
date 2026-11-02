//! Bounded rotating local logs.
//!
//! One daily file, at most seven kept, under `<app root>\logs`. The writer
//! runs on a background thread; the returned guard must be kept alive for the
//! lifetime of the process or buffered lines are dropped at exit.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;

pub fn init(logs_dir: &Path) -> Option<WorkerGuard> {
    if let Err(e) = std::fs::create_dir_all(logs_dir) {
        eprintln!("escape-proxy: cannot create log directory {}: {e}", logs_dir.display());
        return None;
    }
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .max_log_files(7)
        .filename_prefix("escape-proxy")
        .filename_suffix("log")
        .build(logs_dir);
    let appender = match appender {
        Ok(a) => a,
        Err(e) => {
            eprintln!("escape-proxy: cannot open log file in {}: {e}", logs_dir.display());
            return None;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(guard)
}
