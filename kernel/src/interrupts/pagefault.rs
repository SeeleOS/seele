use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::{Cr2, Cr3},
    structures::{
        idt::{InterruptStackFrame, PageFaultErrorCode},
        paging::{OffsetPageTable, Page, PageTable, Translate, mapper::TranslateResult},
    },
};

use crate::{
    interrupts::exception_interrupt::handle_usermode_exception,
    memory::{
        PHYSICAL_MEMORY_OFFSET,
        addrspace::{cow::COW_FLAG, mem_area::Data},
        paging::MAPPER,
        utils::apply_offset,
    },
    misc::{CPU_CORE_CONTEXT, others::is_user_mode},
    process::manager::get_current_process,
    s_println,
    signal::Signal,
    thread::get_current_thread,
};

pub extern "x86-interrupt" fn pagefault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let address = Cr2::read().unwrap();

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
                            crate::memory::addrspace::AddrSpace::file_lazy_cluster_pages(),
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

    actual_pagefault_handler(stack_frame, error_code)
}

fn actual_pagefault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) -> ! {
    let address = Cr2::read().unwrap();
    s_println!(
        "pagefault: addr={:#x} rip={:#x} err={:?}",
        address.as_u64(),
        stack_frame.instruction_pointer.as_u64(),
        error_code
    );

    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::InvalidMemoryAccess);
    }

    let rsp = stack_frame.stack_pointer;
    let rsp_plus_18 = rsp + 0x18u64;
    let rip = stack_frame.instruction_pointer;
    let (active_cr3, _) = Cr3::read();

    let (
        pid,
        process_cr3,
        process_fault_phys,
        process_rsp_phys,
        process_rsp_plus_18_phys,
        process_rip_phys,
    ) = {
        let process_ref = get_current_process();
        let process = process_ref.lock();
        (
            process.pid.0,
            process.addrspace.page_table.frame.start_address().as_u64(),
            process.addrspace.translate_addr(address),
            process.addrspace.translate_addr(rsp),
            process.addrspace.translate_addr(rsp_plus_18),
            process.addrspace.translate_addr(rip),
        )
    };
    let (tid, snapshot_kernel_rsp, thread_kernel_stack_top) = {
        let thread_ref = get_current_thread();
        let mut thread = thread_ref.lock();
        (
            thread.id.0,
            thread.get_appropriate_snapshot().kernel_rsp,
            thread.kernel_stack_top,
        )
    };
    let gs_kernel_stack_top = unsafe {
        if CPU_CORE_CONTEXT.is_null() {
            0
        } else {
            (*CPU_CORE_CONTEXT).gs_kernel_stack_top
        }
    };

    s_println!(
        "kernel pagefault diag: pid={} tid={} active_cr3={:#x} process_cr3={:#x} gs_kstack={:#x} snapshot_krsp={:#x} thread_kstack_top={:#x}",
        pid,
        tid,
        active_cr3.start_address().as_u64(),
        process_cr3,
        gs_kernel_stack_top,
        snapshot_kernel_rsp,
        thread_kernel_stack_top
    );
    s_println!(
        "kernel pagefault translate(proc): fault={:?} rsp={:?} rsp+0x18={:?} rip={:?}",
        process_fault_phys,
        process_rsp_phys,
        process_rsp_plus_18_phys,
        process_rip_phys
    );
    s_println!(
        "kernel pagefault translate(active): fault={:?} rsp={:?} rsp+0x18={:?} rip={:?}",
        active_translate_addr(address),
        active_translate_addr(rsp),
        active_translate_addr(rsp_plus_18),
        active_translate_addr(rip)
    );
    s_println!(
        "kernel pagefault translate(global): fault={:?} rsp={:?} rsp+0x18={:?} rip={:?}",
        global_translate_addr(address),
        global_translate_addr(rsp),
        global_translate_addr(rsp_plus_18),
        global_translate_addr(rip)
    );

    panic!(
        "Kernel page fault. \n {:#?} \n errcode: {:?}",
        stack_frame, error_code
    )
}

fn active_translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    let (active_cr3, _) = Cr3::read();
    let table_addr = VirtAddr::new(apply_offset(active_cr3.start_address().as_u64()));
    let page_table = unsafe { &mut *table_addr.as_mut_ptr::<PageTable>() };
    let mapper = unsafe {
        OffsetPageTable::new(
            page_table,
            VirtAddr::new(*PHYSICAL_MEMORY_OFFSET.get().unwrap()),
        )
    };
    mapper.translate_addr(addr)
}

fn global_translate_addr(addr: VirtAddr) -> Option<PhysAddr> {
    MAPPER.get().unwrap().lock().translate_addr(addr)
}
