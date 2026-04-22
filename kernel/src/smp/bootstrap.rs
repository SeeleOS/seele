use core::{arch::asm, hint::spin_loop, mem::offset_of, ptr};

use bootloader_api::info::MemoryRegionKind;
use conquer_once::spin::OnceCell;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageSize, PageTableFlags, PhysFrame, Size4KiB, Translate,
    },
};

use crate::{
    memory::{
        MEMORY_REGIONS,
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::apply_offset,
    },
    misc::time::Time,
    smp::{
        cpu::{CpuCoreContext, register_application_processor},
        gs::GsContext,
        topology, wait_for_cpu_online, with_current_cpu,
    },
    thread,
};

const AP_STARTUP_MIN_ADDR: u64 = 0x8_000;
const AP_STARTUP_MAX_ADDR: u64 = 0xA_0000;
const AP_WAKE_SPINS: usize = 10_000_000;
const INIT_IPI_DELAY_MS: u64 = 10;
const GDT_CODE32_TEMPLATE: u64 = 0x00cf9a000000ffff;
const GDT_DATA32_TEMPLATE: u64 = 0x00cf92000000ffff;
const GDT_CODE64: u64 = 0x00af9a000000ffff;
const GDT_DATA64: u64 = 0x00cf92000000ffff;
const GDT_USER_DATA64: u64 = 0x00cff3000000ffff;
const GDT_USER_CODE64: u64 = 0x00affb000000ffff;
const AP_DEBUG_HLT: bool = false;
const AP_DEBUG_PROTECTED_HLT: bool = false;
const AP_DEBUG_LONG_HLT: bool = false;
const AP_DEBUG_AFTER_PG_HLT: bool = false;
const IA32_GS_BASE: u32 = 0xc000_0101;
const IA32_KERNEL_GS_BASE: u32 = 0xc000_0102;
const IA32_EFER: u32 = 0xc000_0080;
const IA32_STAR: u32 = 0xc000_0081;
const IA32_LSTAR: u32 = 0xc000_0082;
const IA32_FMASK: u32 = 0xc000_0084;
const EFER_SCE: u64 = 1;
const RFLAGS_INTERRUPT_FLAG: u64 = 1 << 9;
const SYSCALL_KERNEL_CS: u16 = 0x08;
const SYSCALL_SYSRET_BASE: u16 = 0x13;
const TEMP_TSS_SELECTOR: u16 = 0x28;

const PROTECTED_MODE_OFFSET: usize = 37;
const PROTECTED_MODE_JUMP_IMM_OFFSET: usize = 31;
const LONG_MODE_OFFSET: usize = 109;
const GDT_DESCRIPTOR_OFFSET: usize = 208;
const LONG_MODE_JUMP_IMM_OFFSET: usize = 103;
const GDT_OFFSET: usize = 216;
const GDT_KERNEL_CODE_OFFSET: usize = 224;
const GDT_KERNEL_DATA_OFFSET: usize = 232;
const GDT_USER_DATA_OFFSET: usize = 240;
const GDT_USER_CODE_OFFSET: usize = 248;
const GDT_TSS_LOW_OFFSET: usize = 256;
const GDT_TSS_HIGH_OFFSET: usize = 264;
const GDT_CODE32_OFFSET: usize = 272;
const GDT_DATA32_OFFSET: usize = 280;
const CR3_MOFFS_OFFSET: usize = 57;
const TEMP_CR3_OFFSET: usize = 288;
const BSP_CR3_OFFSET: usize = 296;
const STACK_OFFSET: usize = 304;
const ENTRY_OFFSET: usize = 312;
const CPU_CONTEXT_OFFSET: usize = 320;
const TRAMPOLINE_SIZE: usize = 328;

static AP_STARTUP_PAGE: OnceCell<u64> = OnceCell::uninit();
static AP_BOOTSTRAP_CR3: OnceCell<u64> = OnceCell::uninit();

const AP_TRAMPOLINE: [u8; TRAMPOLINE_SIZE] = {
    let mut blob = [0u8; TRAMPOLINE_SIZE];

    // 16-bit entry: disable interrupts, mirror CS to data segments, load
    // the temporary GDT, then enter protected mode.
    blob[0] = 0xfa; // cli
    blob[1] = 0xfc; // cld
    blob[2] = 0xb0;
    blob[3] = b'r';
    blob[4] = 0xe6;
    blob[5] = 0xe9; // debugcon: real-mode entry
    blob[6] = 0x8c;
    blob[7] = 0xc8; // mov ax, cs
    blob[8] = 0x8e;
    blob[9] = 0xd8; // mov ds, ax
    blob[10] = 0x8e;
    blob[11] = 0xc0; // mov es, ax
    blob[12] = 0x8e;
    blob[13] = 0xd0; // mov ss, ax
    blob[14] = 0x0f;
    blob[15] = 0x01;
    blob[16] = 0x16; // lgdt [imm16]
    blob[17] = (GDT_DESCRIPTOR_OFFSET & 0xff) as u8;
    blob[18] = ((GDT_DESCRIPTOR_OFFSET >> 8) & 0xff) as u8;
    blob[19] = 0x0f;
    blob[20] = 0x20;
    blob[21] = 0xc0; // mov eax, cr0
    blob[22] = 0x66;
    blob[23] = 0x83;
    blob[24] = 0xc8;
    blob[25] = 0x01; // or eax, 1
    blob[26] = 0x0f;
    blob[27] = 0x22;
    blob[28] = 0xc0; // mov cr0, eax
    blob[29] = 0x66;
    blob[30] = 0xea; // far jump ptr16:32
    blob[31] = (PROTECTED_MODE_OFFSET & 0xff) as u8;
    blob[32] = ((PROTECTED_MODE_OFFSET >> 8) & 0xff) as u8;
    blob[33] = ((PROTECTED_MODE_OFFSET >> 16) & 0xff) as u8;
    blob[34] = ((PROTECTED_MODE_OFFSET >> 24) & 0xff) as u8;
    blob[35] = 0x38;
    blob[36] = 0x00; // code32 selector

    // 32-bit entry: load the kernel page tables, enable long mode, then jump
    // through a patched far pointer to the 64-bit entry below.
    blob[37] = 0xb8;
    blob[38] = 0x40;
    blob[39] = 0x00;
    blob[40] = 0x00;
    blob[41] = 0x00; // mov eax, 0x10
    blob[42] = 0x8e;
    blob[43] = 0xd8; // mov ds, ax
    blob[44] = 0x8e;
    blob[45] = 0xc0; // mov es, ax
    blob[46] = 0x8e;
    blob[47] = 0xd0; // mov ss, ax
    blob[48] = 0x8e;
    blob[49] = 0xe0; // mov fs, ax
    blob[50] = 0x8e;
    blob[51] = 0xe8; // mov gs, ax
    blob[52] = 0xb0;
    blob[53] = b'p';
    blob[54] = 0xe6;
    blob[55] = 0xe9; // debugcon: protected-mode entry
    blob[56] = 0xa1;
    blob[57] = (TEMP_CR3_OFFSET & 0xff) as u8;
    blob[58] = ((TEMP_CR3_OFFSET >> 8) & 0xff) as u8;
    blob[59] = ((TEMP_CR3_OFFSET >> 16) & 0xff) as u8;
    blob[60] = ((TEMP_CR3_OFFSET >> 24) & 0xff) as u8; // mov eax, [temp_cr3]
    blob[61] = 0x0f;
    blob[62] = 0x22;
    blob[63] = 0xd8; // mov cr3, eax
    blob[64] = 0x0f;
    blob[65] = 0x20;
    blob[66] = 0xe0; // mov eax, cr4
    blob[67] = 0x83;
    blob[68] = 0xc8;
    blob[69] = 0x20; // or eax, 0x20
    blob[70] = 0x0f;
    blob[71] = 0x22;
    blob[72] = 0xe0; // mov cr4, eax
    blob[73] = 0xb9;
    blob[74] = 0x80;
    blob[75] = 0x00;
    blob[76] = 0x00;
    blob[77] = 0xc0; // mov ecx, IA32_EFER
    blob[78] = 0x0f;
    blob[79] = 0x32; // rdmsr
    blob[80] = 0x0d;
    blob[81] = 0x00;
    blob[82] = 0x01;
    blob[83] = 0x00;
    blob[84] = 0x00; // or eax, EFER_LME
    blob[85] = 0x0f;
    blob[86] = 0x30; // wrmsr
    blob[87] = 0x0f;
    blob[88] = 0x20;
    blob[89] = 0xc0; // mov eax, cr0
    blob[90] = 0x0d;
    blob[91] = 0x00;
    blob[92] = 0x00;
    blob[93] = 0x00;
    blob[94] = 0x80; // or eax, CR0_PG
    blob[95] = 0xb0;
    blob[96] = b'g';
    blob[97] = 0xe6;
    blob[98] = 0xe9; // debugcon: paging about to turn on
    blob[99] = 0x0f;
    blob[100] = 0x22;
    blob[101] = 0xc0; // mov cr0, eax
    blob[102] = 0xea;
    blob[107] = 0x08;
    blob[108] = 0x00; // jmp far ptr16:32 to the long-mode entry

    // 64-bit entry: continue on the BSP's page tables that were already loaded
    // in 32-bit mode, then jump into the Rust AP bootstrap.
    blob[109] = 0xb0;
    blob[110] = b'l';
    blob[111] = 0xe6;
    blob[112] = 0xe9; // debugcon: long-mode entry
    blob[113] = 0xb0;
    blob[114] = b'c';
    blob[115] = 0xe6;
    blob[116] = 0xe9; // debugcon: continuing on kernel CR3
    blob[117] = 0x90;
    blob[118] = 0x90;
    blob[119] = 0x90;
    blob[120] = 0x90;
    blob[121] = 0x90;
    blob[122] = 0x90;
    blob[123] = 0x66;
    blob[124] = 0xb8;
    blob[125] = 0x10;
    blob[126] = 0x00; // mov ax, 0x20
    blob[127] = 0x8e;
    blob[128] = 0xd8; // mov ds, ax
    blob[129] = 0x8e;
    blob[130] = 0xc0; // mov es, ax
    blob[131] = 0x8e;
    blob[132] = 0xd0; // mov ss, ax
    blob[133] = 0x31;
    blob[134] = 0xc0; // xor eax, eax
    blob[135] = 0x8e;
    blob[136] = 0xe0; // mov fs, ax
    blob[137] = 0x8e;
    blob[138] = 0xe8; // mov gs, ax
    blob[139] = 0xb0;
    blob[140] = b'd';
    blob[141] = 0xe6;
    blob[142] = 0xe9; // debugcon: data segments loaded
    blob[143] = 0x48;
    blob[144] = 0x8b;
    blob[145] = 0x25;
    blob[146] = 0x9a;
    blob[147] = 0x00;
    blob[148] = 0x00;
    blob[149] = 0x00; // mov rsp, [rip + stack]
    blob[150] = 0xb0;
    blob[151] = b's';
    blob[152] = 0xe6;
    blob[153] = 0xe9; // debugcon: stack loaded
    blob[154] = 0x0f;
    blob[155] = 0x20;
    blob[156] = 0xc0; // mov eax, cr0
    blob[157] = 0x25;
    blob[158] = 0xfb;
    blob[159] = 0xff;
    blob[160] = 0xff;
    blob[161] = 0xff; // and eax, !CR0.EM
    blob[162] = 0x0d;
    blob[163] = 0x02;
    blob[164] = 0x00;
    blob[165] = 0x01;
    blob[166] = 0x00; // or eax, CR0.MP | CR0.WP
    blob[167] = 0x0f;
    blob[168] = 0x22;
    blob[169] = 0xc0; // mov cr0, eax
    blob[170] = 0x0f;
    blob[171] = 0x20;
    blob[172] = 0xe0; // mov eax, cr4
    blob[173] = 0x0d;
    blob[174] = 0x00;
    blob[175] = 0x06;
    blob[176] = 0x00;
    blob[177] = 0x00; // or eax, CR4.OSFXSR | CR4.OSXMMEXCPT
    blob[178] = 0x0f;
    blob[179] = 0x22;
    blob[180] = 0xe0; // mov cr4, eax
    blob[181] = 0x48;
    blob[182] = 0x8b;
    blob[183] = 0x3d;
    blob[184] = 0x84;
    blob[185] = 0x00;
    blob[186] = 0x00;
    blob[187] = 0x00; // mov rdi, [rip + cpu_context]
    blob[188] = 0xb0;
    blob[189] = b'v';
    blob[190] = 0xe6;
    blob[191] = 0xe9; // debugcon: cpu_context loaded
    blob[192] = 0x48;
    blob[193] = 0x8b;
    blob[194] = 0x05;
    blob[195] = 0x71;
    blob[196] = 0x00;
    blob[197] = 0x00;
    blob[198] = 0x00; // mov rax, [rip + entry]
    blob[199] = 0x48;
    blob[200] = 0x83;
    blob[201] = 0xec;
    blob[202] = 0x08; // simulate a call frame without touching memory first
    blob[203] = 0xff;
    blob[204] = 0xe0; // jmp rax
    blob[205] = 0x0f;
    blob[206] = 0x0b; // if Rust returns unexpectedly, fault hard

    // Padding to 8-byte alignment.
    blob[GDT_DESCRIPTOR_OFFSET] = 0x47;
    blob[GDT_DESCRIPTOR_OFFSET + 1] = 0x00; // GDT limit

    // Long mode far pointer selector.
    blob[GDT_DESCRIPTOR_OFFSET + 4] = 0x08;
    blob[GDT_DESCRIPTOR_OFFSET + 5] = 0x00;

    blob
};

pub fn start_application_processors() {
    topology::discover_from_acpi();
    let processors = topology::application_processors();
    if processors.is_empty() {
        log::info!("smp: no application processors discovered");
        return;
    }

    log::info!(
        "smp: discovered {} application processors",
        processors.len()
    );
    let startup_phys = *AP_STARTUP_PAGE.get_or_init(init_ap_startup_page);
    let bootstrap_cr3 = *AP_BOOTSTRAP_CR3.get_or_init(init_ap_bootstrap_cr3);
    let startup_vector = (startup_phys >> 12) as u8;
    log::info!(
        "smp: startup page at {startup_phys:#x}, vector {startup_vector:#x}, bootstrap cr3 {bootstrap_cr3:#x}"
    );

    for processor in processors {
        log::info!(
            "smp: preparing AP {} (cpu index {})",
            processor.apic_id,
            processor.index
        );
        let cpu = register_application_processor(processor.index, processor.apic_id);
        prepare_ap_startup_page(startup_phys, cpu);
        log::info!("smp: waking AP {}", processor.apic_id);
        wake_application_processor(processor.apic_id, startup_vector);

        assert!(
            wait_for_cpu_online(processor.apic_id, AP_WAKE_SPINS),
            "AP {} did not report online",
            processor.apic_id
        );
    }
}

pub fn release_application_processors() {}

fn init_ap_startup_page() -> u64 {
    let phys_addr = find_ap_startup_page();
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(phys_addr));
    let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    let mut mapper = MAPPER.get().unwrap().lock();

    match mapper.translate_addr(page.start_address()) {
        Some(mapped) => {
            assert_eq!(
                mapped,
                frame.start_address(),
                "startup page virtual address already mapped to wrong frame"
            );
        }
        None => unsafe {
            let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
            mapper
                .map_to(page, frame, flags, &mut *frame_allocator)
                .expect("failed to map AP startup page")
                .flush();
        },
    }

    phys_addr
}

fn init_ap_bootstrap_cr3() -> u64 {
    let (pml4, pdpt, pd): (
        PhysFrame<Size4KiB>,
        PhysFrame<Size4KiB>,
        PhysFrame<Size4KiB>,
    ) = {
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        (
            frame_allocator
                .allocate_frame()
                .expect("no frame for AP bootstrap PML4"),
            frame_allocator
                .allocate_frame()
                .expect("no frame for AP bootstrap PDPT"),
            frame_allocator
                .allocate_frame()
                .expect("no frame for AP bootstrap PD"),
        )
    };

    let pml4_phys = pml4.start_address().as_u64();
    let pdpt_phys = pdpt.start_address().as_u64();
    let pd_phys = pd.start_address().as_u64();
    let pml4_ptr = apply_offset(pml4_phys) as *mut u64;
    let pdpt_ptr = apply_offset(pdpt_phys) as *mut u64;
    let pd_ptr = apply_offset(pd_phys) as *mut u64;

    unsafe {
        ptr::write_bytes(pml4_ptr, 0, 512);
        ptr::write_bytes(pdpt_ptr, 0, 512);
        ptr::write_bytes(pd_ptr, 0, 512);
        ptr::write(pml4_ptr, pdpt_phys | 0x3);
        ptr::write(pdpt_ptr, pd_phys | 0x3);
        ptr::write(pd_ptr, 0x83);
    }

    pml4_phys
}

fn find_ap_startup_page() -> u64 {
    let regions = MEMORY_REGIONS
        .get()
        .expect("memory regions not initialized");

    for region in regions.iter() {
        if region.kind != MemoryRegionKind::Usable {
            continue;
        }

        let start = align_up_4k(region.start.max(AP_STARTUP_MIN_ADDR));
        let end = align_down_4k(region.end.min(AP_STARTUP_MAX_ADDR));
        if end.saturating_sub(start) < Size4KiB::SIZE {
            continue;
        }

        return start;
    }

    panic!("failed to find usable low-memory page for AP startup");
}

fn prepare_ap_startup_page(phys_addr: u64, cpu: *mut CpuCoreContext) {
    let dst = apply_offset(phys_addr) as *mut u8;
    let bsp_cr3 = Cr3::read().0.start_address().as_u64();
    let entry = ap_entry_trampoline as *const () as usize as u64;
    let stack_top = unsafe { (*cpu).gs_context.kernel_stack_top };
    let bootstrap_stack_top = phys_addr + Size4KiB::SIZE;
    let gdt_base = (phys_addr + GDT_OFFSET as u64) as u32;
    let long_mode_addr = (phys_addr + LONG_MODE_OFFSET as u64) as u32;

    log::info!(
        "smp: startup trampoline bsp_cr3={bsp_cr3:#x} bootstrap_stack_top={bootstrap_stack_top:#x} stack_top={stack_top:#x} entry={entry:#x}"
    );

    unsafe {
        if AP_DEBUG_HLT {
            ptr::write(dst, 0xfa);
            ptr::write(dst.add(1), 0xf4);
            ptr::write(dst.add(2), 0xeb);
            ptr::write(dst.add(3), 0xfd);
            return;
        }

        ptr::copy_nonoverlapping(AP_TRAMPOLINE.as_ptr(), dst, AP_TRAMPOLINE.len());
        ptr::write_unaligned(dst.add(GDT_DESCRIPTOR_OFFSET + 2) as *mut u32, gdt_base);
        ptr::write_unaligned(dst.add(GDT_KERNEL_CODE_OFFSET) as *mut u64, GDT_CODE64);
        ptr::write_unaligned(dst.add(GDT_KERNEL_DATA_OFFSET) as *mut u64, GDT_DATA64);
        ptr::write_unaligned(dst.add(GDT_USER_DATA_OFFSET) as *mut u64, GDT_USER_DATA64);
        ptr::write_unaligned(dst.add(GDT_USER_CODE_OFFSET) as *mut u64, GDT_USER_CODE64);
        let tss_base = (*cpu).segments.tss as u64;
        let tss_limit =
            (core::mem::size_of::<x86_64::structures::tss::TaskStateSegment>() - 1) as u32;
        ptr::write_unaligned(
            dst.add(GDT_TSS_LOW_OFFSET) as *mut u64,
            encode_tss_descriptor_low(tss_base, tss_limit),
        );
        ptr::write_unaligned(
            dst.add(GDT_TSS_HIGH_OFFSET) as *mut u64,
            encode_tss_descriptor_high(tss_base),
        );
        ptr::write_unaligned(dst.add(GDT_CODE32_OFFSET) as *mut u64, GDT_CODE32_TEMPLATE);
        ptr::write_unaligned(dst.add(GDT_DATA32_OFFSET) as *mut u64, GDT_DATA32_TEMPLATE);
        if AP_DEBUG_PROTECTED_HLT {
            ptr::write(dst.add(PROTECTED_MODE_OFFSET), 0xb8);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 1), 0x40);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 2), 0x00);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 3), 0x00);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 4), 0x00);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 5), 0x8e);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 6), 0xd8);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 7), 0x8e);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 8), 0xc0);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 9), 0x8e);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 10), 0xd0);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 11), 0xfa);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 12), 0xf4);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 13), 0xeb);
            ptr::write(dst.add(PROTECTED_MODE_OFFSET + 14), 0xfd);
            return;
        }

        if AP_DEBUG_LONG_HLT {
            ptr::write(dst.add(LONG_MODE_OFFSET), 0xfa);
            ptr::write(dst.add(LONG_MODE_OFFSET + 1), 0xf4);
            ptr::write(dst.add(LONG_MODE_OFFSET + 2), 0xeb);
            ptr::write(dst.add(LONG_MODE_OFFSET + 3), 0xfd);
            return;
        }
        if AP_DEBUG_AFTER_PG_HLT {
            ptr::write(dst.add(92), 0xfa);
            ptr::write(dst.add(93), 0xf4);
            ptr::write(dst.add(94), 0xeb);
            ptr::write(dst.add(95), 0xfd);
            return;
        }
        ptr::write_unaligned(
            dst.add(PROTECTED_MODE_JUMP_IMM_OFFSET) as *mut u32,
            (phys_addr + PROTECTED_MODE_OFFSET as u64) as u32,
        );
        ptr::write_unaligned(
            dst.add(CR3_MOFFS_OFFSET) as *mut u32,
            (phys_addr + TEMP_CR3_OFFSET as u64) as u32,
        );
        ptr::write_unaligned(
            dst.add(LONG_MODE_JUMP_IMM_OFFSET) as *mut u32,
            long_mode_addr,
        );
        ptr::write_unaligned(dst.add(TEMP_CR3_OFFSET) as *mut u64, bsp_cr3);
        ptr::write_unaligned(dst.add(BSP_CR3_OFFSET) as *mut u64, bsp_cr3);
        ptr::write_unaligned(dst.add(STACK_OFFSET) as *mut u64, bootstrap_stack_top);
        ptr::write_unaligned(dst.add(ENTRY_OFFSET) as *mut u64, entry);
        ptr::write_unaligned(dst.add(CPU_CONTEXT_OFFSET) as *mut u64, cpu as u64);
    }
}

fn wake_application_processor(apic_id: u32, startup_vector: u8) {
    send_init_ipi(apic_id);
    spin_for_ms(INIT_IPI_DELAY_MS);
    send_startup_ipi(apic_id, startup_vector);
}

fn send_init_ipi(apic_id: u32) {
    log::info!("smp: send INIT to AP {}", apic_id);
    unsafe {
        with_current_cpu(|cpu| {
            cpu.local_apic.send_init_ipi(apic_id);
        });
    }
}

fn send_startup_ipi(apic_id: u32, startup_vector: u8) {
    log::info!("smp: send SIPI {startup_vector:#x} to AP {}", apic_id);
    unsafe {
        with_current_cpu(|cpu| {
            cpu.local_apic.send_sipi(startup_vector, apic_id);
        });
    }
}

fn spin_for_ms(milliseconds: u64) {
    let deadline = Time::since_boot().add_ms(milliseconds);
    while Time::since_boot() < deadline {
        spin_loop();
    }
}

const fn align_up_4k(addr: u64) -> u64 {
    (addr + (Size4KiB::SIZE - 1)) & !(Size4KiB::SIZE - 1)
}

const fn align_down_4k(addr: u64) -> u64 {
    addr & !(Size4KiB::SIZE - 1)
}

const fn encode_tss_descriptor_low(base: u64, limit: u32) -> u64 {
    (limit as u64 & 0xffff)
        | ((base & 0x00ff_ffff) << 16)
        | (0x89_u64 << 40)
        | (((limit as u64 >> 16) & 0xf) << 48)
        | (((base >> 24) & 0xff) << 56)
}

const fn encode_tss_descriptor_high(base: u64) -> u64 {
    (base >> 32) & 0xffff_ffff
}

extern "C" fn ap_entry_trampoline(cpu: *mut CpuCoreContext) -> ! {
    ap_main(cpu)
}

fn ap_main(cpu: *mut CpuCoreContext) -> ! {
    unsafe { bootstrap_load_gs_context(cpu) };
    unsafe { bootstrap_load_tss() };
    unsafe { bootstrap_init_systemcall() };

    unsafe {
        ptr::addr_of_mut!((*cpu).online).cast::<u8>().write(1);
    }

    unsafe { bootstrap_jump_to_scheduler() };
}

unsafe fn bootstrap_load_gs_context(cpu: *const CpuCoreContext) {
    let gs_context = unsafe {
        cpu.cast::<u8>()
            .add(offset_of!(CpuCoreContext, gs_context))
            .cast::<GsContext>()
    };
    let gs_base = gs_context as u64;

    unsafe {
        bootstrap_write_msr(IA32_GS_BASE, gs_base);
        bootstrap_write_msr(IA32_KERNEL_GS_BASE, gs_base);
    }
}

unsafe fn bootstrap_write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nostack, preserves_flags)
        );
    }
}

unsafe fn bootstrap_load_tss() {
    unsafe {
        asm!(
            "ltr ax",
            in("ax") TEMP_TSS_SELECTOR,
            options(nostack, preserves_flags)
        );
    }
}

unsafe fn bootstrap_init_systemcall() {
    let efer = unsafe { bootstrap_read_msr(IA32_EFER) } | EFER_SCE;
    let star = ((SYSCALL_SYSRET_BASE as u64) << 48) | ((SYSCALL_KERNEL_CS as u64) << 32);
    let lstar = bootstrap_syscall_entry();

    unsafe {
        bootstrap_write_msr(IA32_EFER, efer);
        bootstrap_write_msr(IA32_FMASK, RFLAGS_INTERRUPT_FLAG);
        bootstrap_write_msr(IA32_STAR, star);
        bootstrap_write_msr(IA32_LSTAR, lstar);
    }
}

unsafe fn bootstrap_read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;

    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );
    }

    ((high as u64) << 32) | (low as u64)
}

fn bootstrap_syscall_entry() -> u64 {
    let entry: u64;

    unsafe {
        asm!(
            "lea {entry}, [rip + {syscall_entry}]",
            entry = out(reg) entry,
            syscall_entry = sym crate::systemcall::entry::syscall_entry,
            options(nostack, preserves_flags)
        );
    }

    entry
}

unsafe fn bootstrap_jump_to_scheduler() -> ! {
    unsafe {
        asm!("jmp {}", sym thread::scheduling::run, options(noreturn));
    }
}
