use x86_64::{
    VirtAddr,
    instructions::interrupts,
    registers::control::Cr2,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::Page,
    },
};

use crate::{
    misc::hlt_loop,
    multitasking::{MANAGER, process::manager::get_current_process},
    println, s_print, s_println,
};

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read().unwrap();

    if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
        actual_pagefault_handler(stack_frame, error_code, address);
    }

    let process = get_current_process();
    let addrspace = &mut process.lock().addrspace;

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

    MANAGER.lock().kill_process(get_current_process());

    unreachable!()
}
