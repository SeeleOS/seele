use core::fmt::{Arguments, Write};

use crate::{graphics::framebuffer::FRAME_BUFFER, misc::serial_print::_print};

use super::state::TERMINAL;

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

    let mut term = TERMINAL.get().unwrap().lock();

    term.write_fmt(args).unwrap();
    term.flush();

    FRAME_BUFFER.get().unwrap().lock().flush();
}
