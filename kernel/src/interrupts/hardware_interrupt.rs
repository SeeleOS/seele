use x86_64::{
    VirtAddr,
    structures::idt::InterruptDescriptorTable,
};

use crate::{
    interrupts::timer::timer_interrupt_handler_wrapper,
    keyboard::ps2::keyboard_interrupt_handler,
    misc::with_cpu_core_context,
};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum HardwareInterrupt {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl HardwareInterrupt {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

pub fn send_eoi() {
    unsafe { with_cpu_core_context(|f| f.local_apic.as_mut().unwrap().end_of_interrupt()) };
}

pub fn init_hardware_interrupts(idt: &mut InterruptDescriptorTable) {
    unsafe {
        idt[HardwareInterrupt::Timer.as_u8()].set_handler_addr(VirtAddr::new(
            timer_interrupt_handler_wrapper as *const () as u64,
        ));
        idt[HardwareInterrupt::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
    };
}
