use core::{
    hint::spin_loop,
    sync::atomic::{AtomicUsize, Ordering},
};

use acpi::sdt::madt::Madt;
use ap_startup::{Context, start_all_aps};
use x86_64::{
    PhysAddr,
    structures::paging::{Mapper, PageSize, PageTableFlags, PhysFrame, Size4KiB},
};

use crate::{
    acpi::{ACPI_TABLE, handler::ACPIHandler},
    interrupts,
    memory::{
        PHYSICAL_MEMORY_OFFSET,
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::page_range_from_addr,
    },
    misc::time::Time,
    smp::{
        cpu::{CpuCoreContext, register_application_processor},
        current_apic_id_raw, set_current_thread, topology, wait_for_cpu_online,
        with_cpu_by_apic_id, with_current_cpu,
    },
    systemcall, thread,
};

const AP_WAKE_SPINS: usize = 10_000_000;

static AP_SCHEDULER_ENTRY_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn start_application_processors() {
    AP_SCHEDULER_ENTRY_COUNT.store(0, Ordering::Release);

    let discover_start = Time::since_boot();
    let current_apic_id = current_apic_id_raw();
    let acpi_tables = ACPI_TABLE
        .get()
        .expect("ACPI must be initialized before SMP");
    let madt = acpi_tables.find_table::<Madt>().expect("ACPI MADT missing");
    topology::discover_from_acpi(current_apic_id, madt.get());
    let discover_done = Time::since_boot();

    let processors = topology::application_processors();
    if processors.is_empty() {
        log::info!("smp: no application processors discovered");
        return;
    }

    log::info!(
        "smp: discovered {} application processors",
        processors.len()
    );

    let register_start = Time::since_boot();
    for processor in processors {
        register_application_processor(processor.index, processor.apic_id);
    }
    let register_done = Time::since_boot();
    log::info!(
        "smp: ap discovery took {}ms, context registration took {}ms",
        discover_done.sub(discover_start).as_milliseconds(),
        register_done.sub(register_start).as_milliseconds()
    );
}

pub fn release_application_processors() {
    let processors = topology::application_processors();
    let release_start = Time::since_boot();

    let acpi_tables = ACPI_TABLE
        .get()
        .expect("ACPI must be initialized before SMP");
    with_current_cpu(|cpu| {
        let ctx = Context {
            current_local_apic: &mut cpu.local_apic,
            acpi_tables,
        };
        start_all_aps::<SeelePlatform, ACPIHandler>(application_processor_main, ctx)
            .expect("failed to start application processors");
    });
    let wakeup_done = Time::since_boot();
    log::info!(
        "smp: ap init/sipi and trampoline confirmation took {}ms",
        wakeup_done.sub(release_start).as_milliseconds()
    );

    let online_wait_start = Time::since_boot();
    for processor in &processors {
        assert!(
            wait_for_cpu_online(processor.apic_id, AP_WAKE_SPINS),
            "AP {} did not report online",
            processor.apic_id
        );
    }
    let online_done = Time::since_boot();
    log::info!(
        "smp: {} application processor(s) reported online in {}ms ({}ms since release)",
        processors.len(),
        online_done.sub(online_wait_start).as_milliseconds(),
        online_done.sub(release_start).as_milliseconds()
    );

    let scheduler_wait_start = Time::since_boot();
    assert!(
        wait_for_ap_scheduler_entries(processors.len(), AP_WAKE_SPINS),
        "not all APs entered the scheduler"
    );
    let scheduler_done = Time::since_boot();
    log::info!(
        "smp: {} application processor(s) entered scheduler in {}ms ({}ms since release)",
        processors.len(),
        scheduler_done.sub(scheduler_wait_start).as_milliseconds(),
        scheduler_done.sub(release_start).as_milliseconds()
    );
}

extern "C" fn application_processor_main() -> ! {
    let apic_id = current_apic_id_raw();
    let context_ptr = with_cpu_by_apic_id(apic_id, |current| current as *const CpuCoreContext);

    crate::smp::cpu::load_segments_for_cpu(unsafe { &*context_ptr });
    systemcall::init();
    interrupts::init_ap();

    with_cpu_by_apic_id(apic_id, |current| {
        current.online.store(true, Ordering::Release);
    });
    set_current_thread(Some(thread::scheduler_thread()));
    AP_SCHEDULER_ENTRY_COUNT.fetch_add(1, Ordering::AcqRel);
    thread::scheduling::run()
}

fn wait_for_ap_scheduler_entries(expected: usize, spins: usize) -> bool {
    for _ in 0..spins {
        if AP_SCHEDULER_ENTRY_COUNT.load(Ordering::Acquire) >= expected {
            return true;
        }
        spin_loop();
    }

    false
}

struct SeelePlatform;

impl ap_startup::platform::Platform for SeelePlatform {
    const STACK_SIZE: usize = 0x40_000;

    fn sleep_us(us: u64) {
        let deadline = Time::since_boot().add_ns(us.saturating_mul(1_000));
        while Time::since_boot() < deadline {
            spin_loop();
        }
    }

    fn phys_to_ptr<T>(phys_addr: u64) -> *mut T {
        (PHYSICAL_MEMORY_OFFSET
            .get()
            .expect("physical memory offset missing")
            + phys_addr) as *mut T
    }

    fn map_memory(virt_addr: u64, phys_addr: u64, size: u64) {
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        let page_range = page_range_from_addr(virt_addr, virt_addr + size - 1);
        let mut mapper = MAPPER.get().expect("mapper missing").lock();
        let mut frame_allocator = FRAME_ALLOCATOR
            .get()
            .expect("frame allocator missing")
            .lock();

        for (index, page) in page_range.enumerate() {
            let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(
                phys_addr + index as u64 * Size4KiB::SIZE,
            ));

            unsafe {
                match mapper.map_to(page, frame, flags, &mut *frame_allocator) {
                    Ok(flush) => flush.flush(),
                    Err(x86_64::structures::paging::mapper::MapToError::PageAlreadyMapped(_)) => {}
                    Err(error) => panic!(
                        "failed to map AP startup page virt={:#x} phys={:#x}: {error:?}",
                        page.start_address().as_u64(),
                        frame.start_address().as_u64(),
                    ),
                }
            }
        }
    }
}
