use log::LevelFilter;

use crate::{graphics::terminal::TERMINAL, println, s_println};

const LEVEL_FILTER: LevelFilter = LevelFilter::Info;

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(LEVEL_FILTER);
}
