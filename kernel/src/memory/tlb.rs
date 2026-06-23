use alloc::vec::Vec;
use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use x86_64::registers::control::Cr3;

static SHOOTDOWN_LOCK: AtomicBool = AtomicBool::new(false);
static SHOOTDOWN_ACKS: AtomicUsize = AtomicUsize::new(0);
const SHOOTDOWN_ACK_SPINS: usize = 10_000_000;

pub fn flush_current() {
    let (frame, flags) = Cr3::read();
    unsafe {
        Cr3::write(frame, flags);
    }
}

pub fn ack_remote_shootdown() {
    SHOOTDOWN_ACKS.fetch_add(1, Ordering::AcqRel);
}

pub fn flush_after_page_table_update(flush_local: bool, loaded_cpu_mask: u64) {
    if flush_local {
        flush_current();
    }
    shootdown_other_cpus(loaded_cpu_mask);
}

fn shootdown_other_cpus(loaded_cpu_mask: u64) {
    if !crate::SMP_ENABLED {
        return;
    }

    lock_shootdown();
    SHOOTDOWN_ACKS.store(0, Ordering::Release);

    let target_apic_ids = online_remote_loaded_apic_ids(loaded_cpu_mask);
    let expected_acks = target_apic_ids.len();
    if expected_acks == 0 {
        unlock_shootdown();
        return;
    }

    for apic_id in target_apic_ids {
        crate::interrupts::hardware_interrupt::shootdown_tlb_cpu(apic_id);
    }

    for _ in 0..SHOOTDOWN_ACK_SPINS {
        if SHOOTDOWN_ACKS.load(Ordering::Acquire) >= expected_acks {
            unlock_shootdown();
            return;
        }
        spin_loop();
    }

    unlock_shootdown();
    panic!(
        "timed out waiting for TLB shootdown ACKs: got {}, expected {}",
        SHOOTDOWN_ACKS.load(Ordering::Acquire),
        expected_acks
    );
}

fn online_remote_loaded_apic_ids(loaded_cpu_mask: u64) -> Vec<u32> {
    let current_apic_id = crate::smp::current_apic_id();
    let mut targets = Vec::new();
    for processor in crate::smp::topology::processors() {
        if processor.apic_id == current_apic_id {
            continue;
        }
        let Some(cpu_bit) = 1u64.checked_shl(processor.index as u32) else {
            continue;
        };
        if loaded_cpu_mask & cpu_bit == 0 {
            continue;
        }
        if !crate::smp::is_cpu_online(processor.apic_id) {
            continue;
        }
        targets.push(processor.apic_id);
    }
    targets
}

fn lock_shootdown() {
    while SHOOTDOWN_LOCK
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        spin_loop();
    }
}

fn unlock_shootdown() {
    SHOOTDOWN_LOCK.store(false, Ordering::Release);
}
