use lazy_static::lazy_static;
use spin::*;
use uart_16550::SerialPort;
use x86_64::instructions::interrupts::{self};

lazy_static! {
    pub static ref ELYSIA_OS: Mutex<OS> = Mutex::new(OS::new());
}

pub struct OS {
    pub serial_port: SerialPort,
}

impl OS {
    pub fn new() -> Self {
        Self {
            serial_port: {
                let mut serial_port = unsafe { SerialPort::new(0x3F8) };
                serial_port.init();
                serial_port
            },
        }
    }
}

pub fn get_os_no_interrupt<F>(func: F)
where
    F: FnOnce(MutexGuard<'static, OS>),
{
    interrupts::without_interrupts(|| func(get_os()));
}

pub fn get_os() -> MutexGuard<'static, OS> {
    ELYSIA_OS.lock()
}
