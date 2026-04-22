use alloc::boxed::Box;
use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use x2apic::lapic::LocalApic;
use x86_64::{
    PrivilegeLevel, VirtAddr,
    instructions::tables::load_tss,
    registers::{
        model_specific::{GsBase, KernelGsBase},
        segmentation::{CS, DS, ES, SS, Segment},
    },
    structures::{
        gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector},
        tss::TaskStateSegment,
    },
};

use crate::{
    interrupts::default_local_apic,
    process::ProcessRef,
    smp::{gs::GsContext, topology},
    thread::{ThreadRef, stack::allocate_kernel_stack},
};

const MAX_XAPIC_IDS: usize = 256;
const KERNEL_CODE_INDEX: u16 = 1;
const KERNEL_DATA_INDEX: u16 = 2;
const USER_DATA_INDEX: u16 = 3;
const USER_CODE_INDEX: u16 = 4;
const TSS_INDEX: u16 = 5;

pub const DOUBLE_FAULT_IST_LOCATION: u16 = 0;
pub const PAGE_FAULT_IST_LOCATION: u16 = 1;
pub const GP_IST_LOCATION: u16 = 2;

static CPU_BY_APIC_ID: [AtomicPtr<CpuCoreContext>; MAX_XAPIC_IDS] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_XAPIC_IDS];

pub struct CpuCoreContext {
    pub index: usize,
    pub apic_id: u32,
    pub is_bsp: bool,
    pub online: AtomicBool,
    pub gs_context: GsContext,
    pub local_apic: LocalApic,
    pub current_thread: Option<ThreadRef>,
    pub current_process: Option<ProcessRef>,
    pub(crate) segments: CpuSegments,
}

pub(crate) struct CpuSegments {
    pub(crate) gdt: GlobalDescriptorTable,
    pub(crate) tss: *mut TaskStateSegment,
}

unsafe impl Send for CpuCoreContext {}
unsafe impl Sync for CpuCoreContext {}

pub fn init_bsp() {
    let apic_id = current_apic_id_raw();
    let ctx = Box::leak(Box::new(CpuCoreContext::new(0, apic_id, true))) as *mut CpuCoreContext;

    register_cpu(ctx);
    load_segments_for_cpu(unsafe { &*ctx });
    topology::register_bsp(apic_id);
}

pub fn load_current_segments() {
    let cpu = with_current_cpu(|cpu| cpu as *mut CpuCoreContext);
    load_segments_for_cpu(unsafe { &*cpu });
}

pub fn load_current_kernel_gs_base() {
    let gs_context = with_current_cpu(|cpu| &cpu.gs_context as *const GsContext);

    unsafe {
        write_gs_bases(gs_context);
    }
}

pub fn load_segments_for_cpu(cpu: &'static CpuCoreContext) {
    unsafe {
        cpu.segments.gdt.load();
        CS::set_reg(kernel_code_selector());
        SS::set_reg(kernel_data_selector());
        DS::set_reg(kernel_data_selector());
        ES::set_reg(kernel_data_selector());
        load_tss(tss_selector());
        write_gs_bases(&cpu.gs_context);
    }
}

fn current_gs_context() -> &'static GsContext {
    let gs_ptr = GsBase::read().as_u64() as *const GsContext;
    assert!(!gs_ptr.is_null(), "current GS context not initialized");
    unsafe { &*gs_ptr }
}

unsafe fn write_gs_bases(gs_context: *const GsContext) {
    let address = unsafe { VirtAddr::from_ptr(&*gs_context) };
    GsBase::write(address);
    KernelGsBase::write(address);
}

pub fn current_apic_id() -> u32 {
    with_current_cpu(|cpu| cpu.apic_id)
}

pub fn current_apic_id_raw() -> u32 {
    (__cpuid(1).ebx >> 24) & 0xff
}

pub fn current_cpu_index() -> usize {
    with_current_cpu(|cpu| cpu.index)
}

pub fn with_current_cpu<R>(f: impl FnOnce(&mut CpuCoreContext) -> R) -> R {
    let gs_context = current_gs_context();
    let ptr = gs_context.cpu_context.cast::<CpuCoreContext>();
    assert!(!ptr.is_null(), "current CPU context not initialized");
    unsafe { f(&mut *ptr) }
}

pub fn current_thread() -> ThreadRef {
    try_current_thread().expect("current thread not initialized")
}

pub fn set_current_thread(thread: Option<ThreadRef>) {
    with_current_cpu(|cpu| cpu.current_thread = thread);
}

pub fn current_process() -> ProcessRef {
    try_current_process().expect("current process not initialized")
}

pub fn set_current_process(process: Option<ProcessRef>) {
    with_current_cpu(|cpu| cpu.current_process = process);
}

pub fn try_current_thread() -> Option<ThreadRef> {
    with_current_cpu(|cpu| cpu.current_thread.clone())
}

pub fn try_current_process() -> Option<ProcessRef> {
    with_current_cpu(|cpu| cpu.current_process.clone())
}

pub fn set_current_kernel_stack(stack_top: u64) {
    with_current_cpu(|cpu| unsafe {
        (*cpu.segments.tss).privilege_stack_table[0] = VirtAddr::new(stack_top);
    });
}

pub fn register_application_processor(index: usize, apic_id: u32) -> *mut CpuCoreContext {
    let ctx =
        Box::leak(Box::new(CpuCoreContext::new(index, apic_id, false))) as *mut CpuCoreContext;
    register_cpu(ctx);
    ctx
}

pub fn with_cpu_by_apic_id<R>(apic_id: u32, f: impl FnOnce(&CpuCoreContext) -> R) -> R {
    let slot = CPU_BY_APIC_ID
        .get(apic_id as usize)
        .unwrap_or_else(|| panic!("unsupported APIC ID {apic_id:#x}"));
    let ptr = slot.load(Ordering::Acquire);
    assert!(
        !ptr.is_null(),
        "CPU core context for APIC ID {apic_id:#x} not initialized"
    );
    unsafe { f(&*ptr) }
}

pub fn mark_current_cpu_online() {
    with_current_cpu(|cpu| {
        cpu.online.store(true, Ordering::Release);
    });
}

pub fn wait_for_cpu_online(apic_id: u32, spins: usize) -> bool {
    for _ in 0..spins {
        if with_cpu_by_apic_id(apic_id, |cpu| cpu.online.load(Ordering::Acquire)) {
            return true;
        }
        core::hint::spin_loop();
    }

    false
}

pub fn kernel_code_selector() -> SegmentSelector {
    SegmentSelector::new(KERNEL_CODE_INDEX, PrivilegeLevel::Ring0)
}

pub fn kernel_data_selector() -> SegmentSelector {
    SegmentSelector::new(KERNEL_DATA_INDEX, PrivilegeLevel::Ring0)
}

pub fn user_code_selector() -> SegmentSelector {
    SegmentSelector::new(USER_CODE_INDEX, PrivilegeLevel::Ring3)
}

pub fn user_data_selector() -> SegmentSelector {
    SegmentSelector::new(USER_DATA_INDEX, PrivilegeLevel::Ring3)
}

pub fn tss_selector() -> SegmentSelector {
    SegmentSelector::new(TSS_INDEX, PrivilegeLevel::Ring0)
}

impl CpuCoreContext {
    fn new(index: usize, apic_id: u32, is_bsp: bool) -> Self {
        let gs_stack_top = allocate_kernel_stack(16).finish().as_u64();
        let tss = Box::leak(Box::new(TaskStateSegment::new()));
        tss.privilege_stack_table[0] = allocate_kernel_stack(16).finish();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_LOCATION as usize] =
            allocate_kernel_stack(5).finish();
        tss.interrupt_stack_table[PAGE_FAULT_IST_LOCATION as usize] =
            allocate_kernel_stack(5).finish();
        tss.interrupt_stack_table[GP_IST_LOCATION as usize] = allocate_kernel_stack(5).finish();

        let tss_ptr = tss as *mut TaskStateSegment;
        let tss_ref = unsafe { &*tss_ptr };

        let mut gdt = GlobalDescriptorTable::new();
        gdt.append(Descriptor::kernel_code_segment());
        gdt.append(Descriptor::kernel_data_segment());
        gdt.append(Descriptor::user_data_segment());
        gdt.append(Descriptor::user_code_segment());
        gdt.append(Descriptor::tss_segment(tss_ref));

        let segments = CpuSegments { gdt, tss: tss_ptr };

        Self {
            index,
            apic_id,
            is_bsp,
            online: AtomicBool::new(is_bsp),
            gs_context: GsContext {
                kernel_stack_top: gs_stack_top,
                user_stack_top: 0,
                cpu_context: core::ptr::null_mut(),
            },
            local_apic: default_local_apic(),
            current_thread: None,
            current_process: None,
            segments,
        }
    }
}

fn register_cpu(ctx: *mut CpuCoreContext) {
    unsafe {
        (*ctx).gs_context.cpu_context = ctx.cast();
    }
    let slot = CPU_BY_APIC_ID
        .get(unsafe { (*ctx).apic_id as usize })
        .unwrap_or_else(|| panic!("unsupported APIC ID {:#x}", unsafe { (*ctx).apic_id }));
    let previous = slot.swap(ctx, Ordering::AcqRel);
    assert!(
        previous.is_null(),
        "CPU core context for APIC ID {:#x} already initialized",
        unsafe { (*ctx).apic_id }
    );
}
