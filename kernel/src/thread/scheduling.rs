use core::{
    arch::naked_asm,
    mem::{offset_of, size_of},
    sync::atomic::{AtomicBool, Ordering},
};

use x86_64::instructions::interrupts::{self, enable_and_hlt, without_interrupts};

use crate::{
    keyboard,
    misc::agent_tty_input,
    misc::mouse,
    misc::snapshot::Snapshot,
    misc::timer::process_expired_process_timers,
    object::linux_anon::expired_timerfd_poll_objects,
    polling::event::PollableEvent,
    signal::process_current_process_signals,
    smp::{current_cpu_index, set_current_kernel_stack, set_current_process, set_current_thread},
    thread::{
        ThreadRef,
        extended_state::{
            clear_active_user_extended_state_ptr, update_active_user_extended_state_ptr_for_thread,
        },
        misc::State,
        scheduler_thread,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        with_thread_manager,
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

fn should_run_global_scheduler_work() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.is_bsp)
}

fn process_deferred_timer_work() {
    process_expired_process_timers();

    with_thread_manager(|manager| {
        manager.process_timed_out_threads();
        manager.wake_ready_pollers();
    });
    for object in expired_timerfd_poll_objects() {
        crate::thread::yielding::wake_pollers_for_object(object, PollableEvent::CanBeRead);
    }
}

pub fn return_to_scheduler(snapshot: &mut Snapshot, snapshot_type: ThreadSnapshotType) {
    let current_ref = crate::thread::get_current_thread();
    let mut current = current_ref.lock();
    let thread_snapshot = current.get_appropriate_snapshot() as *mut ThreadSnapshot;
    let scheduler_snapshot = &mut current.scheduler_snapshot as *mut ThreadSnapshot;
    drop(current);

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
    let current_ref = crate::thread::get_current_thread();
    let mut current = current_ref.lock();
    let thread_snapshot = current.get_appropriate_snapshot() as *mut ThreadSnapshot;
    let scheduler_snapshot = &mut current.scheduler_snapshot as *mut ThreadSnapshot;
    drop(current);

    unsafe { (*scheduler_snapshot).switch_from(Some(&mut *thread_snapshot), None) };

    unreachable!()
}

pub fn run() -> ! {
    loop {
        if should_run_global_scheduler_work() {
            process_deferred_timer_work();
            crate::net::poll();
            keyboard::process_pending_scancodes();
            agent_tty_input::process_pending_input();
            mouse::process_pending_mouse_events();
        }

        let can_run_ready_threads = can_run_ready_threads_on_current_cpu();
        let next_thread = if can_run_ready_threads {
            with_thread_manager(|manager| {
                if should_run_global_scheduler_work() {
                    manager.pop_ready_for_cpu(current_cpu_index())
                } else {
                    manager.pop_user_ready_for_cpu(current_cpu_index())
                }
            })
        } else {
            with_thread_manager(|manager| manager.pop_local_ready_for_cpu(current_cpu_index()))
        };

        if let Some(thread) = next_thread {
            run_ready_thread(thread);
            continue;
        }

        sleep_if_idle();
    }
}

fn run_ready_thread(thread_ref: ThreadRef) {
    let Some((thread_snapshot, scheduler_snapshot)) = without_interrupts(|| {
        let mut thread = thread_ref.lock();
        if !matches!(thread.state, State::Ready) {
            return None;
        }

        let process = thread.parent.clone();
        thread.state = State::Running;
        set_current_thread(Some(thread_ref.clone()));
        update_active_user_extended_state_ptr_for_thread(&mut thread);
        set_current_kernel_stack(thread.kernel_stack_top);

        // The process object can keep the same Arc while execve replaces its
        // address space, so each CPU must refresh CR3 before resuming it.
        process.lock().addrspace.load();
        set_current_process(Some(process));

        Some((
            thread.get_appropriate_snapshot() as *mut ThreadSnapshot,
            &mut thread.scheduler_snapshot as *mut ThreadSnapshot,
        ))
    }) else {
        return;
    };

    unsafe {
        switch_from_scheduler_to_thread(thread_snapshot, scheduler_snapshot);
    };

    after_thread_yield(thread_ref);
}

#[unsafe(naked)]
unsafe extern "C" fn switch_from_scheduler_to_thread(
    thread_snapshot: *mut ThreadSnapshot,
    scheduler_snapshot: *mut ThreadSnapshot,
) {
    naked_asm!(
        "mov [rsi + {K_RSP_OFF}], rsp",
        "sub rsp, {SNAPSHOT_SIZE}",
        "mov [rsp + {R15_OFF}], r15",
        "mov [rsp + {R14_OFF}], r14",
        "mov [rsp + {R13_OFF}], r13",
        "mov [rsp + {R12_OFF}], r12",
        "mov [rsp + {R11_OFF}], r11",
        "mov [rsp + {R10_OFF}], r10",
        "mov [rsp + {R9_OFF}], r9",
        "mov [rsp + {R8_OFF}], r8",
        "mov [rsp + {RDI_OFF}], rdi",
        "mov [rsp + {RSI_OFF}], rsi",
        "mov [rsp + {RBP_OFF}], rbp",
        "mov [rsp + {RBX_OFF}], rbx",
        "mov [rsp + {RDX_OFF}], rdx",
        "mov [rsp + {RCX_OFF}], rcx",
        "mov [rsp + {RAX_OFF}], rax",
        "mov rax, [rsp + {SNAPSHOT_SIZE}]",
        "mov [rsp + {RIP_OFF}], rax",
        "mov rax, cs",
        "mov [rsp + {CS_OFF}], rax",
        "pushfq",
        "pop qword ptr [rsp + {RFLAGS_OFF}]",
        "lea rax, [rsp + {SNAPSHOT_SIZE} + 8]",
        "mov [rsp + {RSP_OFF}], rax",
        "mov rax, ss",
        "mov [rsp + {SS_OFF}], rax",
        "mov rdx, rsp",
        "call {inner}",
        "ud2",
        inner = sym switch_from_scheduler_to_thread_inner,
        K_RSP_OFF = const offset_of!(ThreadSnapshot, kernel_rsp),
        SNAPSHOT_SIZE = const size_of::<Snapshot>(),
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

extern "C" fn switch_from_scheduler_to_thread_inner(
    thread_snapshot: *mut ThreadSnapshot,
    scheduler_snapshot: *mut ThreadSnapshot,
    snapshot_ptr: *mut Snapshot,
) -> ! {
    unsafe {
        (*thread_snapshot).switch_from_presaved_scheduler(
            Some(&mut *scheduler_snapshot),
            Some(&mut *snapshot_ptr),
        );
    }

    unreachable!()
}

fn after_thread_yield(thread_ref: ThreadRef) {
    let state = thread_ref.lock().state.clone();
    if matches!(state, State::Exiting) {
        with_thread_manager(|manager| {
            manager.mark_thread_exited(thread_ref.clone());
            manager.cleanup_exited_threads();
        });
        set_current_thread(Some(scheduler_thread()));
        clear_active_user_extended_state_ptr();
        return;
    }

    let process = {
        let thread = thread_ref.lock();
        thread.parent.clone()
    };
    let should_cleanup = process_current_process_signals(&process);
    if should_cleanup {
        with_thread_manager(|manager| manager.cleanup_exited_threads());
    }

    let state = thread_ref.lock().state.clone();

    match state {
        State::Running => {
            thread_ref.lock().state = State::Ready;
            with_thread_manager(|manager| manager.push_ready_balanced(thread_ref.clone()));
        }
        State::Ready => {
            with_thread_manager(|manager| manager.push_ready_balanced(thread_ref.clone()));
        }
        State::Blocked(_) => {}
        State::Blocking(block_type) => {
            thread_ref.lock().state = State::Blocked(block_type);
        }
        State::Woken => {
            thread_ref.lock().state = State::Ready;
            with_thread_manager(|manager| manager.push_ready_balanced(thread_ref.clone()));
        }
        State::Exiting => {
            with_thread_manager(|manager| manager.mark_thread_exited(thread_ref.clone()));
            with_thread_manager(|manager| manager.cleanup_exited_threads());
        }
        State::Zombie => {
            with_thread_manager(|manager| manager.cleanup_exited_threads());
        }
    }

    set_current_thread(Some(scheduler_thread()));
    clear_active_user_extended_state_ptr();
}

fn sleep_if_idle() {
    interrupts::disable();

    if should_run_global_scheduler_work() {
        process_deferred_timer_work();
    }

    let has_pending_threads = if should_run_global_scheduler_work() {
        with_thread_manager(|manager| manager.has_ready_threads())
    } else {
        false
    };
    let has_pending_work = has_pending_threads
        || (should_run_global_scheduler_work()
            && (keyboard::has_pending_scancodes()
                || agent_tty_input::has_pending_input()
                || mouse::has_pending_events()));

    if has_pending_work {
        interrupts::enable();
    } else {
        enable_and_hlt();
    }
}
