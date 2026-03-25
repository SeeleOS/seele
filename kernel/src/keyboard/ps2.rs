use lazy_static::lazy_static;
use pc_keyboard::{Keyboard, ScancodeSet1, layouts};
use spin::Mutex;
use x86_64::{instructions::port::Port, structures::idt::InterruptStackFrame};

lazy_static! {
    pub static ref _PS2_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            pc_keyboard::HandleControl::Ignore
        ));
}

use crate::{interrupts::hardware_interrupt::send_eoi, keyboard::push_scancode};

pub fn init() {
    let dropped = drain_output_buffer();
    if dropped != 0 {
        log::info!("ps2: dropped {dropped} stale byte(s) left by firmware/bootloader");
    }
}

pub extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut keyboard_port = Port::new(0x60);
    let scancode = unsafe { keyboard_port.read() };
    push_scancode(scancode);
    send_eoi();
}

fn drain_output_buffer() -> usize {
    let mut status_port: Port<u8> = Port::new(0x64);
    let mut data_port: Port<u8> = Port::new(0x60);
    let mut drained = 0;

    while drained < 256 {
        let status = unsafe { status_port.read() };
        if (status & 1) == 0 {
            break;
        }

        let _ = unsafe { data_port.read() };
        drained += 1;
    }

    drained
}
