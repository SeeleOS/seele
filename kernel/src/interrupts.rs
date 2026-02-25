use conquer_once::spin::OnceCell;
use pic8259::ChainedPics;
use spin::Mutex;
use x86_64::{
    instructions::interrupts::{self},
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    driver::init_interrupt_drivers,
    exception_interrupt::init_exception_interrupts,
    hardware_interrupt::{PIC_1_OFFSET, PIC_2_OFFSET, init_hardware_interrupts},
    print, s_println, test,
};
pub mod timer;
use lazy_static::lazy_static;
pub static PICS: OnceCell<Mutex<ChainedPics>> = OnceCell::uninit();

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        init_hardware_interrupts(&mut idt);
        init_interrupt_drivers(&mut idt);
        init_exception_interrupts(&mut idt);

        idt
    };
}

pub fn init() {
    IDT.load();
    PICS.get_or_init(|| unsafe { Mutex::new(ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)) });

    unsafe { PICS.get().unwrap().lock().initialize() };
    interrupts::enable();
}

pub fn print_stackframe_m(stack_frame: InterruptStackFrame) {
    s_println!("{:#?}", stack_frame);
}

pub fn print_stackframe(message: &str, stack_frame: InterruptStackFrame) {
    print!("\n{message}:\n\n");
    print_stackframe_m(stack_frame);
}

// test if breakpoint interrupt will crash the system
test!("Breakpoint interrupt crash", || interrupts::int3());
