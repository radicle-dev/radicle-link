//! Simple `Logger` implementation.
use chrono::prelude::*;
use log::*;

struct Logger {
    level: Level,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let target = if !record.target().is_empty() {
                record.target()
            } else {
                record.module_path().unwrap_or_default()
            };

            println!(
                "{} {:<5} [{}] {}",
                Local::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                record.level(),
                target,
                record.args()
            )
        }
    }

    fn flush(&self) {}
}

pub fn init(level: Level) {
    let logger = Logger { level };

    log::set_boxed_logger(Box::new(logger)).ok();
    log::set_max_level(level.to_level_filter());
}
