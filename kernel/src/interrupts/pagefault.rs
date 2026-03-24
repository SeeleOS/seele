use core::intrinsics::copy_nonoverlapping;

use x86_64::{
    VirtAddr,
    instructions::interrupts,
    registers::control::Cr2,
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::{
            FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
            Size4KiB, Translate,
            mapper::{MappedFrame, TranslateResult},
        },
    },
};

use crate::{
    memory::{addrspace::clone::COW_FLAG, paging::FRAME_ALLOCATOR, utils::apply_offset},
    misc::hlt_loop,
    multitasking::{
        MANAGER,
        process::{manager::get_current_process, new},
        scheduling::return_to_executor_no_save,
    },
    println, s_print, s_println,
};

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    s_println!("A");
    let address = Cr2::read().unwrap();

    let process = get_current_process();
    let addrspace = &mut process.lock().addrspace;

    s_println!("b");
    let page_table = &mut addrspace.page_table.inner;
    s_println!("c");

    if let TranslateResult::Mapped { flags, .. } = page_table.translate(address)
        && flags.contains(COW_FLAG)
    {
        process_cow(address, flags, page_table);
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

fn process_cow(address: VirtAddr, mut flags: PageTableFlags, page_table: &mut OffsetPageTable) {
    s_println!("cow");
    let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
    s_println!("c");

    let new_frame = frame_allocator.allocate_frame().unwrap();
    let new_addr = apply_offset(new_frame.start_address().as_u64());

    let page: Page<Size4KiB> = Page::containing_address(address);

    flags.remove(COW_FLAG);
    flags |= PageTableFlags::WRITABLE;

    let (old_frame, flush) = page_table.unmap(page).unwrap();
    flush.flush();

    unsafe {
        copy_nonoverlapping(
            apply_offset(old_frame.start_address().as_u64()) as *const u8,
            new_addr as *mut u8,
            4096,
        )
    };

    unsafe {
        page_table
            .map_to(page, new_frame, flags, &mut *frame_allocator)
            .unwrap()
            .flush()
    };
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

    return_to_executor_no_save();

    unreachable!()
}
