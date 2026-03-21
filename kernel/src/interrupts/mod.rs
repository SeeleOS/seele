use conquer_once::spin::OnceCell;
use spin::Mutex;
use x2apic::lapic::{LocalApic, LocalApicBuilder, xapic_base};
use x86_64::{
    instructions::interrupts::{self},
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    interrupts::exception_interrupt::init_exception_interrupts,
    interrupts::hardware_interrupt::{PIC_1_OFFSET, PIC_2_OFFSET, init_hardware_interrupts},
    print, test,
};
pub mod exception_interrupt;
pub mod hardware_interrupt;
pub mod timer;
use lazy_static::lazy_static;
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        init_hardware_interrupts(&mut idt);
        init_exception_interrupts(&mut idt);

        idt
    };
}

pub fn init() {
    log::info!("interrupts: init start");
    IDT.load();

    let mut local_apic = LocalApicBuilder::new()
        .timer_vector(32)
        .error_vector(0xFE)
        .spurious_vector(0xFF)
        .build()
        .unwrap();

    unsafe {
        local_apic.enable();
    };

    log::info!("interrupts: init done");
}

pub fn print_stackframe_m(stack_frame: InterruptStackFrame) {
    log::error!("{:#?}", stack_frame);
}

pub fn print_stackframe(message: &str, stack_frame: InterruptStackFrame) {
    print!("\n{message}:\n\n");
    print_stackframe_m(stack_frame);
}

// test if breakpoint interrupt will crash the system
test!("Breakpoint interrupt crash", || interrupts::int3());
