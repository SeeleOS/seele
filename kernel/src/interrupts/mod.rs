use x2apic::lapic::{LocalApic, LocalApicBuilder, TimerDivide, TimerMode, xapic_base};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::{
    interrupts::{
        exception_interrupt::init_exception_interrupts,
        hardware_interrupt::init_hardware_interrupts,
    },
    memory::utils::apply_offset,
    misc::with_cpu_core_context,
    print,
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
    IDT.load();

    unsafe {
        with_cpu_core_context(|f| {
            f.local_apic.as_mut().unwrap().enable();
        });
    };

    io_apic::init();

    log::info!("interrupts: init done");
}

pub fn default_local_apic() -> LocalApic {
    LocalApicBuilder::new()
        .timer_vector(32)
        .error_vector(0xFE)
        .spurious_vector(0xFF)
        .timer_mode(TimerMode::Periodic)
        .timer_divide(TimerDivide::Div1)
        .timer_initial(1_000_000)
        .set_xapic_base(unsafe { apply_offset(xapic_base()) })
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
