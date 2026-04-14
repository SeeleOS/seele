use x86_64::{
    registers::control::Cr2,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::{Page, Translate, mapper::TranslateResult},
    },
};

use crate::{
    interrupts::exception_interrupt::handle_usermode_exception,
    memory::addrspace::{cow::COW_FLAG, mem_area::Data},
    misc::others::is_user_mode,
    process::manager::get_current_process,
};
use seele_sys::signal::Signal;

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read().unwrap();

    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let addrspace = &mut process.addrspace;

    let page_table = &mut addrspace.page_table.inner;

    if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
        && let TranslateResult::Mapped { flags, .. } = page_table.translate(address)
        && flags.contains(COW_FLAG)
    {
        process.addrspace.replace_cow_page(address);
        return;
    }

    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        actual_pagefault_handler(stack_frame, error_code);
    }

    let area = addrspace.get_area(address).cloned();

    match area {
        Some(area) if area.lazy => {
            let is_file_backed = matches!(&area.data, Data::File { .. });
            if is_file_backed {
                addrspace.apply_page_cluster(
                    Page::containing_address(address),
                    area.clone(),
                    crate::memory::addrspace::AddrSpace::file_lazy_cluster_pages(),
                );
            } else {
                addrspace.apply_page(Page::containing_address(address), area.clone());
            }
        }
        _ => actual_pagefault_handler(stack_frame, error_code),
    }
}

fn actual_pagefault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) -> ! {
    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::InvalidMemoryAccess);
    }

    panic!(
        "Kernel page fault. \n {:#?} \n errcode: {:?}",
        stack_frame, error_code
    )
}
