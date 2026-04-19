use core::{
    arch::naked_asm,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
use x86_64::VirtAddr;

use crate::{
    interrupts::hardware_interrupt::send_eoi,
    memory::addrspace::mem_area::Data,
    misc::snapshot::Snapshot,
    process::manager::MANAGER,
    s_println,
    systemcall::handling::pid1_trace_window_active,
    thread::{THREAD_MANAGER, scheduling::return_to_executor, snapshot::ThreadSnapshotType},
};

static PID1_SPIN_RIP: AtomicU64 = AtomicU64::new(0);
static PID1_SPIN_TICKS: AtomicU32 = AtomicU32::new(0);
static PID1_TRACE_SAMPLE_TICKS: AtomicU32 = AtomicU32::new(0);
static PID1_IRQ_SAMPLE_TICKS: AtomicU32 = AtomicU32::new(0);
const PID1_SPIN_SAMPLE_TICKS: u32 = 128;
const PID1_SPIN_REPEAT_TICKS: u32 = 512;
const PID1_TRACE_SAMPLE_PERIOD: u32 = 16;
const PID1_TRACE_SAMPLE_EARLY_TICKS: u32 = 8;

fn log_pid1_timer_irq(snapshot: &Snapshot) {
    if !pid1_trace_window_active() {
        PID1_IRQ_SAMPLE_TICKS.store(0, Ordering::Relaxed);
        return;
    }

    let ticks = PID1_IRQ_SAMPLE_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if ticks > PID1_TRACE_SAMPLE_EARLY_TICKS && ticks % PID1_TRACE_SAMPLE_PERIOD != 0 {
        return;
    }

    s_println!(
        "pid1 timer irq: ticks={} rip={:#x} cs={:#x} rsp={:#x} rflags={:#x}",
        ticks,
        snapshot.rip,
        snapshot.cs,
        snapshot.rsp,
        snapshot.rflags
    );
}

fn log_pid1_userspace_spin(snapshot: &Snapshot) {
    let Some(current_thread) = THREAD_MANAGER.get().unwrap().lock().current.clone() else {
        return;
    };

    let process_ref = current_thread.lock().parent.clone();
    let mut process = process_ref.lock();
    if process.pid.0 != 1 {
        PID1_SPIN_RIP.store(0, Ordering::Relaxed);
        PID1_SPIN_TICKS.store(0, Ordering::Relaxed);
        return;
    }

    let rip = snapshot.rip;
    let ticks = if PID1_SPIN_RIP.load(Ordering::Relaxed) == rip {
        PID1_SPIN_TICKS.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        PID1_SPIN_RIP.store(rip, Ordering::Relaxed);
        PID1_SPIN_TICKS.store(1, Ordering::Relaxed);
        1
    };

    let trace_sample = if pid1_trace_window_active() {
        let trace_ticks = PID1_TRACE_SAMPLE_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        trace_ticks % PID1_TRACE_SAMPLE_PERIOD == 0
    } else {
        PID1_TRACE_SAMPLE_TICKS.store(0, Ordering::Relaxed);
        false
    };

    if !trace_sample && ticks != PID1_SPIN_SAMPLE_TICKS && ticks % PID1_SPIN_REPEAT_TICKS != 0 {
        return;
    }

    let describe_area = |addr: u64, process: &mut crate::process::Process| {
        let Some(area) = process.addrspace.get_area(VirtAddr::new(addr)) else {
            return alloc::format!("none@{addr:#x}");
        };

        match &area.data {
            Data::Normal => alloc::format!(
                "anon[{:#x}-{:#x})@{:#x}",
                area.start.as_u64(),
                area.end.as_u64(),
                addr
            ),
            Data::Shared { .. } => alloc::format!(
                "shared[{:#x}-{:#x})@{:#x}",
                area.start.as_u64(),
                area.end.as_u64(),
                addr
            ),
            Data::File { file, .. } => alloc::format!(
                "file:{}[{:#x}-{:#x})@{:#x}",
                file.path().as_string(),
                area.start.as_u64(),
                area.end.as_u64(),
                addr
            ),
        }
    };

    let rip_area = describe_area(snapshot.rip, &mut process);
    let rsp_area = describe_area(snapshot.rsp, &mut process);
    let kind = if trace_sample {
        "pid1 timer sample"
    } else {
        "pid1 userspace spin"
    };
    s_println!(
        "{}: ticks={} rip={:#x} rsp={:#x} rflags={:#x} rip_area={} rsp_area={}",
        kind,
        ticks,
        snapshot.rip,
        snapshot.rsp,
        snapshot.rflags,
        rip_area,
        rsp_area
    );
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn timer_interrupt_handler_wrapper() {
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
            "push r15", // 它是最后一个入栈的，地址最低
        "mov rdi, rsp",
        "call {handler}",
        // If the handler returns, restore registers and iretq.
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
        "iretq",
        handler = sym timer_interrupt_handler, // 符号绑定
    )
}

pub extern "C" fn timer_interrupt_handler(snapshot: &mut Snapshot) {
    log_pid1_timer_irq(snapshot);
    send_eoi();

    {
        let manager = MANAGER.lock();
        for process in manager.processes.values() {
            process.lock().process_timers();
        }
    }

    THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .process_timed_out_threads();

    // Don't preempt kernel mode; it can corrupt in-flight kernel snapshots.
    if (snapshot.cs & 0x3) == 0 {
        return;
    }

    log_pid1_userspace_spin(snapshot);

    return_to_executor(snapshot, ThreadSnapshotType::Thread);

    unreachable!();
}
