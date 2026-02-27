use alloc::collections::vec_deque::VecDeque;
use crossbeam_queue::ArrayQueue;
use lazy_static::lazy_static;
use pc_keyboard::{DecodedKey, KeyCode, Keyboard, ScancodeSet1, layouts};
use spin::{Mutex, MutexGuard};
use x86_64::instructions::port::Port;

lazy_static! {
    pub static ref _PS2_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            pc_keyboard::HandleControl::Ignore
        ));
}

use crate::{
    driver::{
        Driver, InterruptDriver,
        keyboard::scancode_processing::{KEYBOARD_QUEUE, add_scancode},
    },
    hardware_interrupt::{HardwareInterrupt, HardwareInterruptHandler},
    print, register_hardware_interrupt,
};

pub trait KeyboardDriver: Driver {
    fn handle_key(key: DecodedKey) {
        match key {
            DecodedKey::Unicode(character) => KEYBOARD_QUEUE
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock()
                .push_back(character as u8),
            DecodedKey::RawKey(key) => Self::handle_raw_key(key),
        }
    }

    fn handle_raw_key(_key: KeyCode) {
        // TODO: handle delete keys and enter and all that stuffs
    }
}

pub struct PS2KeyboardDriver;

impl Driver for PS2KeyboardDriver {}

impl InterruptDriver for PS2KeyboardDriver {
    fn idt_init(idt: &mut x86_64::structures::idt::InterruptDescriptorTable) {
        register_hardware_interrupt!(idt, HardwareInterrupt::Keyboard, Self);
    }
}

impl KeyboardDriver for PS2KeyboardDriver {}

pub fn get_keyboard() -> MutexGuard<'static, Keyboard<layouts::Us104Key, ScancodeSet1>> {
    _PS2_KEYBOARD.lock()
}

impl HardwareInterruptHandler for PS2KeyboardDriver {
    const HARDWARE_INTERRUPT: crate::hardware_interrupt::HardwareInterrupt =
        HardwareInterrupt::Keyboard;

    fn handle_hardware_interrupt_unwrapped(
        _stack_frame: x86_64::structures::idt::InterruptStackFrame,
    ) {
        let mut keyboard_port = Port::new(0x60);
        let scancode = unsafe { keyboard_port.read() };

        add_scancode(scancode);
    }
}
