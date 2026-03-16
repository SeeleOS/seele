use core::fmt::{Arguments, Write};

use alloc::fmt::format;

use crate::{
    graphics::{framebuffer::FRAME_BUFFER, terminal::state::DEFAULT_TERMINAL},
    misc::serial_print::_print,
    object::traits::Writable,
};

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::graphics::terminal::term_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn term_print(args: Arguments) {
    _print(args);

    DEFAULT_TERMINAL
        .get()
        .unwrap()
        .lock()
        .write(format(args).as_bytes());
}
