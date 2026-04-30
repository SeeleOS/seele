use x86_64::{
    registers::control::Cr2,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::{Page, Translate, mapper::TranslateResult},
    },
};

use crate::{
    interrupts::exception_interrupt::handle_usermode_exception,
    memory::addrspace::{AddrSpace, cow::COW_FLAG, mem_area::Data},
    misc::others::is_user_mode,
    process::manager::get_current_process,
    signal::Signal,
};

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read().unwrap();
    log_boot_debug_pagefault("enter", address.as_u64(), error_code);

    let handled = {
        let process_ref = get_current_process();
        let mut process = process_ref.lock();
        let addrspace = &mut process.addrspace;
        let page_table = &mut addrspace.page_table.inner;

        if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
            && let TranslateResult::Mapped { flags, .. } = page_table.translate(address)
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
        log_boot_debug_pagefault("handled", address.as_u64(), error_code);
        return;
    }

    log_boot_debug_pagefault("unhandled", address.as_u64(), error_code);
    actual_pagefault_handler(stack_frame, error_code)
}

fn actual_pagefault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) -> ! {
    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::SIGSEGV);
    }

    panic!(
        "Kernel page fault. \n {:#?} \n errcode: {:?}",
        stack_frame, error_code
    )
}

fn log_boot_debug_pagefault(stage: &str, address: u64, error_code: PageFaultErrorCode) {
    crate::process::misc::with_current_process(|process| {
        let Some(command) = process.command_line.first() else {
            return;
        };
        let Some(name) = command.rsplit('/').next() else {
            return;
        };
        if !matches!(name, "systemd-random-seed" | "systemd-sysusers") {
            return;
        }

        let tid = crate::thread::get_current_thread().lock().id.0;
        crate::s_println!(
            "bootpf stage={} comm={} pid={} tid={} addr={:#x} err={:?}",
            stage,
            name,
            process.pid.0,
            tid,
            address,
            error_code
        );
    });
}
