use core::{arch::naked_asm, mem::offset_of};

use x86_64::{
    registers::control::Cr2,
    structures::{
        idt::PageFaultErrorCode,
        paging::{Page, mapper::TranslateResult},
    },
};

use crate::{
    interrupts::exception_interrupt::handle_usermode_exception,
    memory::addrspace::{AddrSpace, cow::COW_FLAG, mem_area::Data},
    misc::snapshot::SnapshotWithErrorCode,
    process::manager::get_current_process,
    signal::Signal,
    smp::gs::GsContext,
};

pub extern "C" fn pagefault_handler(
    snapshot: &mut SnapshotWithErrorCode,
    error_code: PageFaultErrorCode,
    from_user: u64,
) {
    let address = Cr2::read().unwrap();

    let handled = {
        let process_ref = get_current_process();
        let mut process = process_ref.lock();
        let addrspace = &mut process.addrspace;

        if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
            && let TranslateResult::Mapped { flags, .. } = addrspace.page_table.translate(address)
            && flags.contains(COW_FLAG)
        {
            process.addrspace.replace_cow_page(address);
            true
        } else if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            false
        } else {
            match addrspace.get_area(address).cloned() {
                Some(area) if area.lazy => {
                    let is_file_backed = matches!(&area.data, Data::File { .. });
                    if is_file_backed {
                        addrspace.apply_page_cluster(
                            Page::containing_address(address),
                            area.clone(),
                            AddrSpace::file_lazy_cluster_pages(),
                        );
                    } else {
                        addrspace.apply_page(Page::containing_address(address), area.clone());
                    }
                    true
                }
                _ => false,
            }
        }
    };

    if handled {
        return;
    }

    let snapshot = snapshot.as_snapshot();
    if from_user != 0 {
        handle_usermode_exception(&snapshot, Signal::SIGSEGV);
    }

    panic!(
        "Kernel page fault. \n {:#?} \n fault address: {:?} \n errcode: {:?}",
        snapshot, address, error_code
    )
}

#[unsafe(naked)]
pub extern "C" fn pagefault_user_wrapper() {
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
        "xor edx, edx",
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "setnz dl",
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
        "mov rdi, rsp",
        "mov rsi, [rsp + {ERR_OFF}]",
        "call {inner}",
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "jz 5f",
        "mov r8, qword ptr gs:[{ACTIVE_EXT_STATE_OFF}]",
        "test r8, r8",
        "jz 4f",
        "cmp qword ptr gs:[{USES_XSAVE_OFF}], 0",
        "je 3f",
        "mov eax, dword ptr gs:[{XCR0_LOW_OFF}]",
        "mov edx, dword ptr gs:[{XCR0_HIGH_OFF}]",
        "xrstor64 [r8]",
        "jmp 4f",
        "3:",
        "fxrstor64 [r8]",
        "4:",
        "mov qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 0",
        "swapgs",
        "5:",
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
        "add rsp, 8",
        "iretq",
        inner = sym pagefault_handler,
        ERR_OFF = const offset_of!(SnapshotWithErrorCode, error_code),
        CS_OFF = const offset_of!(SnapshotWithErrorCode, cs),
        ACTIVE_EXT_STATE_OFF = const offset_of!(GsContext, active_user_extended_state),
        ACTIVE_EXT_STATE_SAVED_OFF =
            const offset_of!(GsContext, active_user_extended_state_saved),
        USES_XSAVE_OFF = const offset_of!(GsContext, extended_state_uses_xsave),
        XCR0_LOW_OFF = const offset_of!(GsContext, extended_state_xcr0),
        XCR0_HIGH_OFF = const offset_of!(GsContext, extended_state_xcr0) + 4,
    )
}
