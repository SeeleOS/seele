use core::{
    hint::spin_loop,
    sync::atomic::{AtomicUsize, Ordering},
};

use limine::mp::Cpu;

use crate::{
    boot, interrupts,
    smp::{
        cpu::{CpuCoreContext, register_application_processor},
        set_current_thread, topology, wait_for_cpu_online, with_cpu_by_apic_id,
    },
    systemcall, thread,
};

const AP_WAKE_SPINS: usize = 10_000_000;

static AP_SCHEDULER_ENTRY_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn start_application_processors() {
    AP_SCHEDULER_ENTRY_COUNT.store(0, Ordering::Release);

    let response = boot::mp_response();
    topology::discover_from_limine(response);

    let processors = topology::application_processors();
    if processors.is_empty() {
        log::info!("smp: no application processors discovered");
        return;
    }

    log::info!(
        "smp: discovered {} application processors",
        processors.len()
    );

    for processor in processors {
        let cpu = register_application_processor(processor.index, processor.apic_id);
        let limine_cpu = response
            .cpus()
            .iter()
            .copied()
            .find(|entry| entry.lapic_id == processor.apic_id)
            .expect("limine cpu entry missing for discovered AP");

        limine_cpu
            .extra
            .store(cpu as usize as u64, Ordering::Release);
    }
}

pub fn release_application_processors() {
    let response = boot::mp_response();
    let processors = topology::application_processors();

    for processor in &processors {
        let limine_cpu = response
            .cpus()
            .iter()
            .copied()
            .find(|entry| entry.lapic_id == processor.apic_id)
            .expect("limine cpu entry missing for released AP");
        limine_cpu.goto_address.write(application_processor_main);
    }

    for processor in &processors {
        assert!(
            wait_for_cpu_online(processor.apic_id, AP_WAKE_SPINS),
            "AP {} did not report online",
            processor.apic_id
        );
    }

    assert!(
        wait_for_ap_scheduler_entries(processors.len(), AP_WAKE_SPINS),
        "not all APs entered the scheduler"
    );
    log::info!(
        "smp: {} application processor(s) entered scheduler",
        processors.len()
    );
}

unsafe extern "C" fn application_processor_main(cpu: &Cpu) -> ! {
    let context_ptr = cpu.extra.load(Ordering::Acquire) as usize as *mut CpuCoreContext;
    assert!(!context_ptr.is_null(), "limine ap context missing");

    crate::smp::cpu::load_segments_for_cpu(unsafe { &*context_ptr });
    systemcall::init();
    interrupts::init_ap();

    with_cpu_by_apic_id(cpu.lapic_id, |current| {
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
