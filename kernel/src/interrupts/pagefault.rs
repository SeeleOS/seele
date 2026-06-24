use core::{arch::naked_asm, mem::offset_of};

use x86_64::{
    VirtAddr,
    registers::control::Cr2,
    structures::{
        idt::PageFaultErrorCode,
        paging::{Page, PageSize, Size4KiB, mapper::TranslateResult},
    },
};

use crate::{
    interrupts::exception_interrupt::handle_usermode_exception,
    memory::{
        addrspace::{AddrSpace, cow::COW_FLAG, mem_area::Data},
        protection::Protection,
    },
    misc::{
        profile::{self, ProfileCategory},
        snapshot::SnapshotWithErrorCode,
    },
    process::manager::get_current_process,
    signal::Signal,
    smp::gs::GsContext,
};

const STACK_GUARD_GAP_PAGES: u64 = 256;

pub extern "C" fn pagefault_handler(
    snapshot: &mut SnapshotWithErrorCode,
    error_code: PageFaultErrorCode,
    from_user: u64,
) {
    let fault_start = profile::scope_start();
    let address = Cr2::read().unwrap();

    let fault_signal = {
        let process_ref = get_current_process();
        let mut process = process_ref.lock();
        let addrspace = &mut process.addrspace;
        let lookup_start = profile::scope_start();

        if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
            && let TranslateResult::Mapped { flags, .. } = addrspace.page_table.translate(address)
            && flags.contains(COW_FLAG)
        {
            profile::record(ProfileCategory::PageFaultLookup, lookup_start);
            let resolve_start = profile::scope_start();
            process.addrspace.replace_cow_page(address);
            profile::record(ProfileCategory::PageFaultResolve, resolve_start);
            profile::record(ProfileCategory::PageFaultCow, resolve_start);
            None
        } else if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
            profile::record(ProfileCategory::PageFaultLookup, lookup_start);
            Some(Signal::SIGSEGV)
        } else {
            let mut area = addrspace.get_area(address).cloned();
            if area.is_none() {
                let fault_page = Page::<Size4KiB>::containing_address(address);
                let fault_page_start = fault_page.start_address();
                if let Some(index) = addrspace.memory_areas.iter().position(|area| {
                    area.grows_down
                        && fault_page_start < area.start
                        && fault_page_start + Size4KiB::SIZE == area.start
                }) {
                    let guard_start = VirtAddr::new(
                        fault_page_start
                            .as_u64()
                            .saturating_sub(STACK_GUARD_GAP_PAGES * Size4KiB::SIZE),
                    );
                    let guard_blocked =
                        addrspace
                            .memory_areas
                            .iter()
                            .enumerate()
                            .any(|(other_index, other)| {
                                other_index != index
                                    && other.start < fault_page_start
                                    && other.end > guard_start
                            });

                    if !guard_blocked {
                        addrspace.memory_areas[index].start = fault_page_start;
                        addrspace.last_area_index = Some(index);
                        area = addrspace.memory_areas.get(index).cloned();
                    }
                }
            }
            profile::record(ProfileCategory::PageFaultLookup, lookup_start);
            match area {
                Some(area)
                    if area.lazy && fault_allowed_by_protection(error_code, area.protection) =>
                {
                    let resolve_start = profile::scope_start();
                    let is_file_backed = matches!(&area.data, Data::File { .. });
                    if is_file_backed {
                        let beyond_file = if let Data::File {
                            file_bytes,
                            zero_fill_after_file,
                            ..
                        } = &area.data
                        {
                            let page_start =
                                Page::<Size4KiB>::containing_address(address).start_address();
                            let page_offset = page_start.as_u64() - area.start.as_u64();
                            !*zero_fill_after_file && page_offset >= *file_bytes
                        } else {
                            false
                        };
                        if beyond_file {
                            profile::record(ProfileCategory::PageFaultResolve, resolve_start);
                            Some(Signal::SIGBUS)
                        } else {
                            let applied = addrspace.apply_page_cluster(
                                Page::containing_address(address),
                                area.clone(),
                                AddrSpace::file_lazy_cluster_pages(),
                            );
                            profile::record(ProfileCategory::PageFaultResolve, resolve_start);
                            profile::record(ProfileCategory::PageFaultFileLazy, resolve_start);
                            profile::record_cycles(
                                ProfileCategory::PageFaultFileLazyCacheLookup,
                                applied.file_lazy_stats.cache_lookup_cycles,
                            );
                            profile::record_cycles(
                                ProfileCategory::PageFaultFileLazyCacheLoad,
                                applied.file_lazy_stats.cache_load_cycles,
                            );
                            profile::record_cycles(
                                ProfileCategory::PageFaultFileLazyMap,
                                applied.file_lazy_stats.map_cycles,
                            );
                            profile::record_cycles(
                                ProfileCategory::PageFaultFileLazyCopy,
                                applied.file_lazy_stats.copy_cycles,
                            );
                            profile::record_file_lazy_fault(profile::FileLazyFaultRecord {
                                cluster_pages_loaded: applied.file_lazy_stats.cluster_pages_loaded,
                                cache_hits: applied.file_lazy_stats.cache_hits,
                                cache_misses: applied.file_lazy_stats.cache_misses,
                                cache_lookup_cycles: applied.file_lazy_stats.cache_lookup_cycles,
                                cache_load_cycles: applied.file_lazy_stats.cache_load_cycles,
                                map_cycles: applied.file_lazy_stats.map_cycles,
                                copy_cycles: applied.file_lazy_stats.copy_cycles,
                            });
                            None
                        }
                    } else {
                        addrspace.apply_page(Page::containing_address(address), area.clone());
                        profile::record(ProfileCategory::PageFaultResolve, resolve_start);
                        profile::record(ProfileCategory::PageFaultAnonLazy, resolve_start);
                        None
                    }
                }
                _ => Some(Signal::SIGSEGV),
            }
        }
    };

    if fault_signal.is_none() {
        profile::record(ProfileCategory::PageFault, fault_start);
        return;
    }

    profile::record(ProfileCategory::PageFault, fault_start);

    let snapshot = snapshot.as_snapshot();
    if from_user != 0 {
        handle_usermode_exception(&snapshot, fault_signal.unwrap_or(Signal::SIGSEGV));
    }

    panic!(
        "Kernel page fault. \n {:#?} \n fault address: {:?} \n errcode: {:?}",
        snapshot, address, error_code
    )
}

fn fault_allowed_by_protection(error_code: PageFaultErrorCode, protection: Protection) -> bool {
    if error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH) {
        return protection.contains(Protection::EXEC);
    }
    if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
        return protection.contains(Protection::WRITE);
    }
    protection.contains(Protection::READ)
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
