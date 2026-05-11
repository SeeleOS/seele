use core::{arch::naked_asm, mem::offset_of};

use x2apic::lapic::IpiAllShorthand;
use x86_64::{VirtAddr, structures::idt::InterruptDescriptorTable};

use crate::{
    interrupts::timer::timer_interrupt_handler_wrapper,
    keyboard::ps2::keyboard_interrupt_handler,
    misc::{mouse::mouse_interrupt_handler, snapshot::Snapshot},
    smp::{gs::GsContext, with_current_cpu},
};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum HardwareInterrupt {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Mouse,
    SchedulerWake = PIC_1_OFFSET + 15,
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
    unsafe { with_current_cpu(|cpu| cpu.local_apic.end_of_interrupt()) };
}

pub fn wake_scheduler_cpus() {
    unsafe {
        with_current_cpu(|cpu| {
            cpu.local_apic.send_ipi_all(
                HardwareInterrupt::SchedulerWake.as_u8(),
                IpiAllShorthand::AllExcludingSelf,
            );
        });
    }
}

pub fn init_hardware_interrupts(idt: &mut InterruptDescriptorTable) {
    unsafe {
        idt[HardwareInterrupt::Timer.as_u8()].set_handler_addr(VirtAddr::new(
            timer_interrupt_handler_wrapper as *const () as u64,
        ));
        idt[HardwareInterrupt::Keyboard.as_u8()].set_handler_addr(VirtAddr::new(
            keyboard_interrupt_wrapper as *const () as u64,
        ));
        idt[HardwareInterrupt::Mouse.as_u8()]
            .set_handler_addr(VirtAddr::new(mouse_interrupt_wrapper as *const () as u64));
        idt[HardwareInterrupt::SchedulerWake.as_u8()].set_handler_addr(VirtAddr::new(
            scheduler_wake_interrupt_wrapper as *const () as u64,
        ))
    };
}

extern "C" fn scheduler_wake_interrupt_handler() {
    send_eoi();
}

macro_rules! define_user_safe_irq_wrapper {
    ($wrapper:ident, $handler:path) => {
        #[unsafe(naked)]
        extern "C" fn $wrapper() {
            naked_asm!(
                "push rax",
                "push rcx",
                "push rdx",
                "push rbx",
                "push rbp",
                "push rsi",
                "push rdi",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "push r12",
                "push r13",
                "push r14",
                "push r15",
                "test qword ptr [rsp + {CS_OFF}], 0x3",
                "jz 0f",
                "swapgs",
                "mov r8, qword ptr gs:[{ACTIVE_EXT_STATE_OFF}]",
                "test r8, r8",
                "jz 0f",
                "cmp qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 0",
                "jne 0f",
                "cmp qword ptr gs:[{USES_XSAVE_OFF}], 0",
                "je 1f",
                "mov eax, dword ptr gs:[{XCR0_LOW_OFF}]",
                "mov edx, dword ptr gs:[{XCR0_HIGH_OFF}]",
                "xsave64 [r8]",
                "jmp 2f",
                "1:",
                "fxsave64 [r8]",
                "2:",
                "mov qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 1",
                "0:",
                "call {handler}",
                "test qword ptr [rsp + {CS_OFF}], 0x3",
                "jz 3f",
                "mov r8, qword ptr gs:[{ACTIVE_EXT_STATE_OFF}]",
                "test r8, r8",
                "jz 4f",
                "cmp qword ptr gs:[{USES_XSAVE_OFF}], 0",
                "je 5f",
                "mov eax, dword ptr gs:[{XCR0_LOW_OFF}]",
                "mov edx, dword ptr gs:[{XCR0_HIGH_OFF}]",
                "xrstor64 [r8]",
                "jmp 4f",
                "5:",
                "fxrstor64 [r8]",
                "4:",
                "mov qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 0",
                "swapgs",
                "3:",
                "pop r15",
                "pop r14",
                "pop r13",
                "pop r12",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rdi",
                "pop rsi",
                "pop rbp",
                "pop rbx",
                "pop rdx",
                "pop rcx",
                "pop rax",
                "iretq",
                handler = sym $handler,
                CS_OFF = const offset_of!(Snapshot, cs),
                ACTIVE_EXT_STATE_OFF = const offset_of!(GsContext, active_user_extended_state),
                ACTIVE_EXT_STATE_SAVED_OFF =
                    const offset_of!(GsContext, active_user_extended_state_saved),
                USES_XSAVE_OFF = const offset_of!(GsContext, extended_state_uses_xsave),
                XCR0_LOW_OFF = const offset_of!(GsContext, extended_state_xcr0),
                XCR0_HIGH_OFF = const offset_of!(GsContext, extended_state_xcr0) + 4,
            )
        }
    };
}

define_user_safe_irq_wrapper!(keyboard_interrupt_wrapper, keyboard_interrupt_handler);
define_user_safe_irq_wrapper!(mouse_interrupt_wrapper, mouse_interrupt_handler);
define_user_safe_irq_wrapper!(
    scheduler_wake_interrupt_wrapper,
    scheduler_wake_interrupt_handler
);
