use core::fmt::Arguments;

use alloc::fmt::format;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    misc::serial_print::_print, object::traits::Writable, terminal::state::DEFAULT_TERMINAL,
};

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
        _print(args);

        DEFAULT_TERMINAL
            .get()
            .unwrap()
            .lock()
            .write(format(args).as_bytes())
            .unwrap();
    });
}
