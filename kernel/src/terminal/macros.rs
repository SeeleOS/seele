use core::fmt::Arguments;

use alloc::fmt::format;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{misc::serial_print::_print, terminal::state::DEFAULT_TERMINAL};

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::terminal::term_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn term_print(args: Arguments) {
    without_interrupts(|| {
        let rendered = format(args);
        _print(format_args!("{rendered}"));
        if !rendered.is_empty() {
            DEFAULT_TERMINAL
                .get()
                .unwrap()
                .lock()
                .write_screen_text(&rendered);
        }
    });
}
