use core::fmt::{self, Write};

use conquer_once::spin::OnceCell;
use spin::Mutex;
use uart_16550::SerialPort;

#[macro_export]
macro_rules! s_print {
    ($($arg:tt)*) => ($crate::misc::serial_print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! s_println {
    () => ($crate::s_print!("\n"));
    ($($arg:tt)*) => ($crate::s_print!("{}\n", format_args!($($arg)*)));
}

pub static SERIAL_PORT: OnceCell<Mutex<SerialPort>> = OnceCell::uninit();

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    SERIAL_PORT
        .get_or_init(|| {
            let mut serial_port = unsafe { SerialPort::new(0x3F8) };
            serial_port.init();
            Mutex::new(serial_port)
        })
        .lock()
        .write_fmt(args)
        .expect("Failed to print to serial port")
}
