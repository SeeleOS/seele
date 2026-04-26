use alloc::sync::Arc;
use core::{
    arch::naked_asm,
    mem::offset_of,
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
};

use x86_64::instructions::interrupts::{self, enable_and_hlt, without_interrupts};

use crate::{
    keyboard,
    misc::agent_tty_input,
    misc::mouse,
    misc::snapshot::Snapshot,
    misc::timer::process_expired_process_timers,
    object::linux_anon::wake_expired_timerfds_with_manager,
    process::manager::MANAGER,
    signal::process_current_process_signals,
    smp::{set_current_kernel_stack, set_current_process, set_current_thread, try_current_process},
    thread::{
        THREAD_MANAGER, ThreadRef,
        misc::State,
        scheduler_thread,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
    },
};

static AP_TASK_SCHEDULING_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable_ap_task_scheduling() {
    AP_TASK_SCHEDULING_ENABLED.store(true, Ordering::Release);
}

fn can_run_ready_threads_on_current_cpu() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.is_bsp)
        || AP_TASK_SCHEDULING_ENABLED.load(Ordering::Acquire)
}

fn process_deferred_timer_work() {
    process_expired_process_timers();

    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    manager.process_timed_out_threads();
    wake_expired_timerfds_with_manager(&mut manager);
    manager.wake_ready_pollers();
}

pub fn return_to_scheduler(snapshot: &mut Snapshot, snapshot_type: ThreadSnapshotType) {
    let (thread_snapshot, scheduler_snapshot) = {
        let _manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = crate::thread::get_current_thread();
        let mut current = current_ref.lock();

        (
            current.get_appropriate_snapshot() as *mut ThreadSnapshot,
            &mut current.scheduler_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe {
        (*thread_snapshot).snapshot_type = snapshot_type;
        (*scheduler_snapshot).switch_from(Some(&mut *thread_snapshot), Some(snapshot));
    }
}

#[unsafe(naked)]
pub extern "C" fn return_to_scheduler_from_current() {
    naked_asm!(
        "sub rsp, {FRAME_SIZE}",
        "mov [rsp + {TMP_RAX_OFF}], rax",
        "mov [rsp + {TMP_RDI_OFF}], rdi",
        "mov [rsp + {R15_OFF}], r15",
        "mov [rsp + {R14_OFF}], r14",
        "mov [rsp + {R13_OFF}], r13",
        "mov [rsp + {R12_OFF}], r12",
        "mov [rsp + {R11_OFF}], r11",
        "mov [rsp + {R10_OFF}], r10",
        "mov [rsp + {R9_OFF}], r9",
        "mov [rsp + {R8_OFF}], r8",
        "mov rax, [rsp + {TMP_RDI_OFF}]",
        "mov [rsp + {RDI_OFF}], rax",
        "mov [rsp + {RSI_OFF}], rsi",
        "mov [rsp + {RBP_OFF}], rbp",
        "mov [rsp + {RBX_OFF}], rbx",
        "mov [rsp + {RDX_OFF}], rdx",
        "mov [rsp + {RCX_OFF}], rcx",
        "mov rax, [rsp + {TMP_RAX_OFF}]",
        "mov [rsp + {RAX_OFF}], rax",
        "mov rax, [rsp + {RET_ADDR_OFF}]",
        "mov [rsp + {RIP_OFF}], rax",
        "mov rax, cs",
        "mov [rsp + {CS_OFF}], rax",
        "pushfq",
        "pop qword ptr [rsp + {RFLAGS_OFF}]",
        "lea rax, [rsp + {RET_RSP_OFF}]",
        "mov [rsp + {RSP_OFF}], rax",
        "mov rax, ss",
        "mov [rsp + {SS_OFF}], rax",
        "mov rdi, rsp",
        "call {inner}",
        "ud2",
        inner = sym return_to_scheduler_from_current_inner,
        FRAME_SIZE = const size_of::<Snapshot>() + 16,
        TMP_RAX_OFF = const size_of::<Snapshot>(),
        TMP_RDI_OFF = const size_of::<Snapshot>() + 8,
        RET_ADDR_OFF = const size_of::<Snapshot>() + 16,
        RET_RSP_OFF = const size_of::<Snapshot>() + 24,
        R15_OFF = const offset_of!(Snapshot, r15),
        R14_OFF = const offset_of!(Snapshot, r14),
        R13_OFF = const offset_of!(Snapshot, r13),
        R12_OFF = const offset_of!(Snapshot, r12),
        R11_OFF = const offset_of!(Snapshot, r11),
        R10_OFF = const offset_of!(Snapshot, r10),
        R9_OFF = const offset_of!(Snapshot, r9),
        R8_OFF = const offset_of!(Snapshot, r8),
        RDI_OFF = const offset_of!(Snapshot, rdi),
        RSI_OFF = const offset_of!(Snapshot, rsi),
        RBP_OFF = const offset_of!(Snapshot, rbp),
        RBX_OFF = const offset_of!(Snapshot, rbx),
        RDX_OFF = const offset_of!(Snapshot, rdx),
        RCX_OFF = const offset_of!(Snapshot, rcx),
        RAX_OFF = const offset_of!(Snapshot, rax),
        RIP_OFF = const offset_of!(Snapshot, rip),
        CS_OFF = const offset_of!(Snapshot, cs),
        RFLAGS_OFF = const offset_of!(Snapshot, rflags),
        RSP_OFF = const offset_of!(Snapshot, rsp),
        SS_OFF = const offset_of!(Snapshot, ss),
    )
}

extern "C" fn return_to_scheduler_from_current_inner(snapshot_ptr: *mut Snapshot) -> ! {
    log::trace!("return_to_scheduler_from_current");
    let snapshot = unsafe { &mut *snapshot_ptr };

    return_to_scheduler(snapshot, ThreadSnapshotType::Kernel);

    unreachable!()
}

pub fn return_to_scheduler_no_save() -> ! {
    log::trace!("return_to_scheduler_no_save");
    let (thread_snapshot, scheduler_snapshot) = {
        let _manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = crate::thread::get_current_thread();
        let mut current = current_ref.lock();

        (
            current.get_appropriate_snapshot() as *mut ThreadSnapshot,
            &mut current.scheduler_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe { (*scheduler_snapshot).switch_from(Some(&mut *thread_snapshot), None) };

    unreachable!()
}

pub fn run() -> ! {
    loop {
        process_deferred_timer_work();
        crate::net::poll();
        keyboard::process_pending_scancodes();
        agent_tty_input::process_pending_input();
        mouse::process_pending_mouse_events();

        let next_thread = if can_run_ready_threads_on_current_cpu() {
            let mut manager = THREAD_MANAGER.get().unwrap().lock();
            manager.pop_ready()
        } else {
            None
        };

        if let Some(thread) = next_thread {
            run_ready_thread(thread);
            continue;
        }

        sleep_if_idle();
    }
}

fn run_ready_thread(thread_ref: ThreadRef) {
    let (thread_snapshot, scheduler_snapshot) = without_interrupts(|| {
        let _manager = THREAD_MANAGER.get().unwrap().lock();
        let mut thread = thread_ref.lock();
        let process = thread.parent.clone();
        thread.state = State::Running;
        set_current_thread(Some(thread_ref.clone()));
        set_current_kernel_stack(thread.kernel_stack_top);

        if try_current_process()
            .as_ref()
            .is_some_and(|current| !Arc::ptr_eq(current, &process))
        {
            MANAGER.lock().load_process(process);
        } else {
            set_current_process(Some(process));
        }

        (
            thread.get_appropriate_snapshot() as *mut ThreadSnapshot,
            &mut thread.scheduler_snapshot as *mut ThreadSnapshot,
        )
    });

    unsafe {
        (*thread_snapshot).switch_from(
            Some(&mut *scheduler_snapshot),
            Some(&mut Snapshot::from_current()),
        )
    };

    after_thread_yield(thread_ref);
}

fn after_thread_yield(thread_ref: ThreadRef) {
    let process = {
        let thread = thread_ref.lock();
        thread.parent.clone()
    };
    let should_cleanup = process_current_process_signals(&process);
    if should_cleanup {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();
    }

    let state = thread_ref.lock().state.clone();

    match state {
        State::Running => {
            thread_ref.lock().state = State::Ready;
            THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .push_ready(thread_ref.clone());
        }
        State::Ready => {
            THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .push_ready(thread_ref.clone());
        }
        State::Blocked(_) => {}
        State::Zombie => {
            THREAD_MANAGER
                .get()
                .unwrap()
                .lock()
                .cleanup_exited_threads();
        }
    }

    set_current_thread(Some(scheduler_thread()));
}

fn sleep_if_idle() {
    interrupts::disable();

    process_deferred_timer_work();

    let has_pending_work = keyboard::has_pending_scancodes()
        || agent_tty_input::has_pending_input()
        || mouse::has_pending_events()
        || { THREAD_MANAGER.get().unwrap().lock().has_ready_threads() };

    if has_pending_work {
        interrupts::enable();
    } else {
        enable_and_hlt();
    }
}
