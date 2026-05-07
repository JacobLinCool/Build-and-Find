//! Tiny logging facade. Records are formatted as a single line and written
//! to stderr. The active level is global state set once at startup.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "error" => Some(Level::Error),
            "warn" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            "trace" => Some(Level::Trace),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn level() -> Level {
    match LEVEL.load(Ordering::Relaxed) {
        0 => Level::Error,
        1 => Level::Warn,
        2 => Level::Info,
        3 => Level::Debug,
        _ => Level::Trace,
    }
}

fn enabled(target: Level) -> bool {
    target as u8 <= LEVEL.load(Ordering::Relaxed)
}

fn emit(level: Level, msg: &str) {
    if !enabled(level) {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let date = crate::http::types::format_http_date(secs);
    eprintln!("[{}] {} {}", date, level.as_str(), msg);
}

pub fn error(msg: &str) {
    emit(Level::Error, msg);
}
pub fn warn(msg: &str) {
    emit(Level::Warn, msg);
}
pub fn info(msg: &str) {
    emit(Level::Info, msg);
}
pub fn debug(msg: &str) {
    emit(Level::Debug, msg);
}
pub fn trace(msg: &str) {
    emit(Level::Trace, msg);
}
