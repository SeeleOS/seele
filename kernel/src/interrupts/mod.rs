use x2apic::lapic::{LocalApic, LocalApicBuilder, TimerDivide, TimerMode, xapic_base};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::{
    interrupts::{
        exception_interrupt::init_exception_interrupts,
        hardware_interrupt::init_hardware_interrupts,
    },
    memory::mmio::map_mmio,
    print,
    smp::with_current_cpu,
};
pub mod exception_interrupt;
pub mod hardware_interrupt;
pub mod io_apic;
pub mod pagefault;
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
    init_local();
    io_apic::init();

    log::info!("interrupts: init done");
}

pub fn init_ap() {
    init_local();
}

fn init_local() {
    IDT.load();

    unsafe {
        with_current_cpu(|cpu| {
            cpu.local_apic.enable();
        });
    };
}

pub fn default_local_apic() -> LocalApic {
    LocalApicBuilder::new()
        .timer_vector(32)
        .error_vector(0xFE)
        .spurious_vector(0xFF)
        .timer_mode(TimerMode::Periodic)
        .timer_divide(TimerDivide::Div1)
        .timer_initial(1_000_000)
        .set_xapic_base(map_mmio(unsafe { xapic_base() }, 4096))
        .build()
        .unwrap()
}

pub fn print_stackframe_m(stack_frame: InterruptStackFrame) {
    log::error!("{:#?}", stack_frame);
}

pub fn print_stackframe(message: &str, stack_frame: InterruptStackFrame) {
    print!("\n{message}:\n\n");
    print_stackframe_m(stack_frame);
}
