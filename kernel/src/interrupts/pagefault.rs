use x86_64::{
    VirtAddr,
    registers::control::Cr2,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::{Page, Translate, mapper::TranslateResult},
    },
};

use crate::{
    memory::addrspace::cow::COW_FLAG,
    process::manager::{MANAGER, get_current_process},
    s_println,
    thread::scheduling::return_to_executor_no_save,
};

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read().unwrap();

    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let addrspace = &mut process.addrspace;

    let page_table = &mut addrspace.page_table.inner;

    if let TranslateResult::Mapped { flags, .. } = page_table.translate(address)
        && flags.contains(COW_FLAG)
    {
        process.addrspace.replace_cow_page(address);
        return;
    }

    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        actual_pagefault_handler(stack_frame, error_code, address);
    }

    match addrspace.get_area(address) {
        Some(area) if area.lazy => {
            addrspace.apply_page(Page::containing_address(address), *area);
        }
        _ => actual_pagefault_handler(stack_frame, error_code, address),
    }
}

fn actual_pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
    address: VirtAddr,
) -> ! {
    s_println!("pagefaulted on {:?}", get_current_process().lock().pid);
    s_println!("error code {:?}", error_code);
    s_println!("stack frame {:#?}", stack_frame);
    s_println!("address {:?}", address);

    MANAGER.lock().terminate_process(get_current_process());

    return_to_executor_no_save();

    unreachable!()
}
