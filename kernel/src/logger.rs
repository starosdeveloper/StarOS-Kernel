// SPDX-License-Identifier: MIT OR Apache-2.0
//! No-op logger for kernel
//!
//! Empty logger implementation that LTO will optimize away completely

use log::{Log, Metadata, Record};

struct NoOpLogger;

impl Log for NoOpLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        false
    }

    fn log(&self, _record: &Record) {
        // Empty - LTO will remove this
    }

    fn flush(&self) {
        // Empty - LTO will remove this
    }
}

static LOGGER: NoOpLogger = NoOpLogger;

/// Initialize the no-op logger
pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Off);
}
