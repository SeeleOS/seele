use log::{Level, LevelFilter};

use crate::{graphics::terminal::TERMINAL, println, s_println};
use owo_colors::OwoColorize;

const LEVEL_FILTER: LevelFilter = LevelFilter::Info;

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let content = record.args();
            match record.level() {
                Level::Error => {
                    s_println!(
                        "{} {}",
                        " Error ".white().bold().on_red(),
                        content.red().bold()
                    );

                    println!(
                        "{} {}",
                        " Error ".white().bold().on_red(),
                        content.red().bold()
                    );
                }
                Level::Warn => println!(
                    "{} {}",
                    "  Warn ".white().bold().on_yellow(),
                    content.yellow().bold()
                ),
                Level::Info => {
                    println!("{} {}", "  Info ".white().bold().on_bright_blue(), content)
                }
                Level::Debug => {
                    println!("{} {}", " Debug ".white().bold().on_bright_black(), content)
                }
                Level::Trace => {
                    println!(
                        "{} {}",
                        " Trace ".white().on_bright_black(),
                        content.bright_black()
                    )
                }
            }
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(LEVEL_FILTER);
}
