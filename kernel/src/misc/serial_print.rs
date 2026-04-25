use core::fmt::{self, Write};

use conquer_once::spin::OnceCell;
use spin::Mutex;
use uart_16550::{Config, Uart16550Tty, backend::PioBackend};
use x86_64::instructions::interrupts::without_interrupts;

#[macro_export]
macro_rules! s_print {
    ($($arg:tt)*) => ($crate::misc::serial_print::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! s_println {
    () => ($crate::s_print!("\n"));
    ($($arg:tt)*) => ($crate::s_print!("{}\n", format_args!($($arg)*)));
}

pub static SERIAL_PORT: OnceCell<Mutex<Uart16550Tty<PioBackend>>> = OnceCell::uninit();

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    without_interrupts(|| {
        SERIAL_PORT
            .get_or_init(|| {
                let serial_port = unsafe {
                    Uart16550Tty::new_port(0x3F8, Config::default())
                        .expect("failed to initialize serial port")
                };
                Mutex::new(serial_port)
            })
            .lock()
            .write_fmt(args)
            .expect("Failed to print to serial port")
    })
}
