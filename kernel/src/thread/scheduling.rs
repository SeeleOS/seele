use core::{
    arch::naked_asm,
    mem::{offset_of, size_of},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use x86_64::instructions::interrupts::{self, enable_and_hlt, without_interrupts};

use crate::{
    keyboard,
    misc::agent_tty_input,
    misc::mouse,
    misc::profile::{self, ProfileCategory},
    misc::snapshot::Snapshot,
    misc::time::Time,
    misc::timer::{next_process_timer_deadline, process_expired_process_timers},
    object::linux_anon::{expired_timerfd_poll_objects, next_timerfd_poll_deadline},
    polling::event::PollableEvent,
    s_print,
    signal::process_current_process_signals,
    smp::{
        current_apic_id, current_cpu_index, set_current_kernel_stack, set_current_process,
        set_current_thread,
    },
    thread::{
        ThreadRef,
        extended_state::{
            clear_active_user_extended_state_ptr, update_active_user_extended_state_ptr_for_thread,
        },
        misc::State,
        scheduler_thread,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        thread::DEFAULT_USER_TIMESLICE_NS,
        with_thread_manager,
    },
};

static AP_TASK_SCHEDULING_ENABLED: AtomicBool = AtomicBool::new(false);
static PROFILE_REPORT_TICK: AtomicU32 = AtomicU32::new(0);

pub fn enable_ap_task_scheduling() {
    AP_TASK_SCHEDULING_ENABLED.store(true, Ordering::Release);
}

pub fn request_current_cpu_resched() {
    crate::smp::with_current_cpu(|cpu| {
        cpu.need_resched.store(true, Ordering::Release);
    });
}

pub fn request_remote_resched(apic_id: u32) {
    crate::smp::with_cpu_by_apic_id(apic_id, |cpu| {
        cpu.need_resched.store(true, Ordering::Release);
    });
}

pub fn request_all_cpus_resched() {
    for processor in crate::smp::topology::processors() {
        if processor.apic_id == current_apic_id() {
            request_current_cpu_resched();
        } else {
            request_remote_resched(processor.apic_id);
        }
    }
}

pub fn take_current_cpu_resched_request() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.need_resched.swap(false, Ordering::AcqRel))
}

pub fn current_cpu_has_resched_request() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.need_resched.load(Ordering::Acquire))
}

pub fn reload_thread_timeslice(thread: &mut crate::thread::thread::Thread) {
    thread.timeslice_remaining_ns = DEFAULT_USER_TIMESLICE_NS;
}

pub fn note_user_mode_resume(now_ns: u64) {
    crate::smp::with_current_cpu(|cpu| {
        cpu.last_timer_tick_ns.store(now_ns, Ordering::Release);
    });
}

fn note_thread_run_start(start_cycles: u64) {
    crate::smp::with_current_cpu(|cpu| {
        cpu.thread_run_start_cycles
            .store(start_cycles, Ordering::Release);
    });
}

fn take_thread_run_start() -> u64 {
    crate::smp::with_current_cpu(|cpu| cpu.thread_run_start_cycles.swap(0, Ordering::AcqRel))
}

pub fn consume_current_thread_timeslice(now_ns: u64) -> bool {
    let elapsed_ns = crate::smp::with_current_cpu(|cpu| {
        let previous = cpu.last_timer_tick_ns.swap(now_ns, Ordering::AcqRel);
        now_ns.saturating_sub(previous)
    });

    if elapsed_ns == 0 {
        return false;
    }

    let thread_ref = crate::thread::get_current_thread();
    let mut thread = thread_ref.lock();
    if !matches!(
        thread.get_appropriate_snapshot().snapshot_type,
        ThreadSnapshotType::Thread
    ) {
        return false;
    }

    let exhausted = elapsed_ns >= thread.timeslice_remaining_ns;
    thread.timeslice_remaining_ns = thread.timeslice_remaining_ns.saturating_sub(elapsed_ns);
    exhausted
}

fn can_run_ready_threads_on_current_cpu() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.is_bsp)
        || AP_TASK_SCHEDULING_ENABLED.load(Ordering::Acquire)
}

fn should_run_global_scheduler_work() -> bool {
    crate::smp::with_current_cpu(|cpu| cpu.is_bsp)
}

#[derive(Clone, Copy, Debug, Default)]
struct SchedulerDeferredWork {
    thread_deadline: Option<Time>,
    process_timer_deadline: Option<Time>,
    timerfd_deadline: Option<Time>,
    pollers_dirty: bool,
    input_pending: bool,
}

impl SchedulerDeferredWork {
    fn next_deadline(self) -> Option<Time> {
        [
            self.thread_deadline,
            self.process_timer_deadline,
            self.timerfd_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn has_due_deadline(self, now: Time) -> bool {
        self.next_deadline().is_some_and(|deadline| deadline <= now)
    }

    fn has_background_reason_to_spin(self) -> bool {
        self.pollers_dirty || self.input_pending
    }

    fn should_keep_cpu_awake(self, now: Time) -> bool {
        self.has_due_deadline(now) || self.has_background_reason_to_spin()
    }
}

fn pending_input_work() -> bool {
    keyboard::has_pending_scancodes()
        || agent_tty_input::has_pending_input()
        || mouse::has_pending_events()
}

fn scheduler_deferred_work_snapshot() -> SchedulerDeferredWork {
    let (thread_deadline, pollers_dirty) =
        with_thread_manager(|manager| (manager.next_timed_deadline(), manager.pollers_dirty()));

    SchedulerDeferredWork {
        thread_deadline,
        process_timer_deadline: next_process_timer_deadline(),
        timerfd_deadline: next_timerfd_poll_deadline(),
        pollers_dirty,
        input_pending: pending_input_work(),
    }
}

fn process_deferred_timer_work(snapshot: SchedulerDeferredWork) {
    let now = Time::since_boot();
    let timerfd_objects = snapshot
        .timerfd_deadline
        .is_some_and(|deadline| deadline <= now)
        .then(expired_timerfd_poll_objects)
        .unwrap_or_default();
    if snapshot
        .process_timer_deadline
        .is_some_and(|deadline| deadline <= now)
    {
        process_expired_process_timers(now);
    }

    with_thread_manager(|manager| {
        if manager.has_timed_out_threads_due(now) {
            manager.process_timed_out_threads();
        }
        if snapshot.pollers_dirty && manager.has_blocked_pollers() {
            manager.wake_ready_pollers();
        }
    });
    for object in timerfd_objects {
        crate::thread::yielding::wake_pollers_for_object(object, PollableEvent::CanBeRead);
    }
}

fn maybe_report_profile() {
    if PROFILE_REPORT_TICK.fetch_add(1, Ordering::Relaxed) & 0x3f == 0 {
        profile::maybe_report();
    }
}

pub fn return_to_scheduler(snapshot: &mut Snapshot, snapshot_type: ThreadSnapshotType) {
    s_print!("S");
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
    s_print!("N");
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
        let scheduler_start = profile::scope_start();
        if should_run_global_scheduler_work() {
            let deferred_work = scheduler_deferred_work_snapshot();
            let timer_work_start = profile::scope_start();
            if deferred_work.has_due_deadline(Time::since_boot()) || deferred_work.pollers_dirty {
                process_deferred_timer_work(deferred_work);
            }
            profile::record(ProfileCategory::TimerWork, timer_work_start);

            let net_poll_start = profile::scope_start();
            crate::net::poll();
            profile::record(ProfileCategory::NetPoll, net_poll_start);

            let other_kernel_start = profile::scope_start();
            if deferred_work.input_pending {
                keyboard::process_pending_scancodes();
                agent_tty_input::process_pending_input();
                mouse::process_pending_mouse_events();
            }
            profile::record(ProfileCategory::OtherKernel, other_kernel_start);
        }

        let select_start = profile::scope_start();
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
        profile::record(ProfileCategory::SchedulerSelect, select_start);

        if let Some(thread) = next_thread {
            let dispatch_start = profile::scope_start();
            let thread_run_cycles = run_ready_thread(thread);
            let dispatch_end = profile::scope_start();
            let total_dispatch_cycles = dispatch_end.saturating_sub(dispatch_start);
            if thread_run_cycles != 0 {
                profile::record_cycles(ProfileCategory::ThreadRunWindow, thread_run_cycles);
            }
            let scheduler_switch_cycles = total_dispatch_cycles.saturating_sub(thread_run_cycles);
            profile::record_cycles(ProfileCategory::SchedulerSwitch, scheduler_switch_cycles);
            let scheduler_work_cycles = dispatch_end
                .saturating_sub(scheduler_start)
                .saturating_sub(thread_run_cycles);
            profile::record_cycles(ProfileCategory::SchedulerWork, scheduler_work_cycles);
            maybe_report_profile();
            continue;
        }

        let idle_start = profile::scope_start();
        sleep_if_idle();
        profile::record(ProfileCategory::Idle, idle_start);
        profile::record(ProfileCategory::SchedulerWork, scheduler_start);
        maybe_report_profile();
    }
}

fn run_ready_thread(thread_ref: ThreadRef) -> u64 {
    let dispatch_prepare_start = profile::scope_start();
    let Some((thread_snapshot, scheduler_snapshot)) = without_interrupts(|| {
        let mut thread = thread_ref.lock();
        if !matches!(thread.state, State::Ready) {
            return None;
        }

        let process = thread.parent.clone();
        thread.state = State::Running;
        if matches!(
            thread.get_appropriate_snapshot().snapshot_type,
            ThreadSnapshotType::Thread
        ) {
            reload_thread_timeslice(&mut thread);
        }
        set_current_thread(Some(thread_ref.clone()));
        update_active_user_extended_state_ptr_for_thread(&mut thread);
        set_current_kernel_stack(thread.kernel_stack_top);
        crate::smp::with_current_cpu(|cpu| {
            cpu.need_resched.store(false, Ordering::Release);
        });
        note_user_mode_resume(Time::since_boot().as_nanoseconds());

        // The process object can keep the same Arc while execve replaces its
        // address space, so each CPU must refresh CR3 before resuming it.
        process.lock().addrspace.load();
        set_current_process(Some(process));

        Some((
            thread.get_appropriate_snapshot() as *mut ThreadSnapshot,
            &mut thread.scheduler_snapshot as *mut ThreadSnapshot,
        ))
    }) else {
        return 0;
    };
    profile::record(ProfileCategory::SchedulerDispatch, dispatch_prepare_start);

    note_thread_run_start(profile::scope_start());
    unsafe {
        switch_from_scheduler_to_thread(thread_snapshot, scheduler_snapshot);
    };
    let thread_run_start = take_thread_run_start();
    let thread_run_cycles = if thread_run_start == 0 {
        0
    } else {
        profile::scope_start().saturating_sub(thread_run_start)
    };

    let after_yield_start = profile::scope_start();
    after_thread_yield(thread_ref);
    profile::record(ProfileCategory::SchedulerAfterYield, after_yield_start);
    thread_run_cycles
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
    let had_resched_request = take_current_cpu_resched_request();
    let deferred_work = should_run_global_scheduler_work().then(scheduler_deferred_work_snapshot);
    let now = Time::since_boot();

    if let Some(deferred_work) = deferred_work
        && (deferred_work.has_due_deadline(now) || deferred_work.pollers_dirty)
    {
        process_deferred_timer_work(deferred_work);
    }

    let has_pending_threads = if should_run_global_scheduler_work() {
        with_thread_manager(|manager| manager.has_ready_threads())
    } else {
        false
    };
    let has_pending_work = has_pending_threads
        || deferred_work.is_some_and(|work| work.should_keep_cpu_awake(now))
        || had_resched_request;

    if has_pending_work {
        interrupts::enable();
    } else {
        enable_and_hlt();
    }
}

#[cfg(test)]
mod tests {
    use super::{SchedulerDeferredWork, reload_thread_timeslice, take_current_cpu_resched_request};
    use crate::misc::time::Time;
    use crate::thread::{scheduling::request_current_cpu_resched, thread::Thread};

    crate::test!(
        reload_thread_timeslice_resets_default_budget,
        "reload thread timeslice resets default budget",
        || {
            let mut thread = Thread::default();
            thread.timeslice_remaining_ns = 1;
            reload_thread_timeslice(&mut thread);
            assert_eq!(
                thread.timeslice_remaining_ns,
                crate::thread::thread::DEFAULT_USER_TIMESLICE_NS
            );
        }
    );

    crate::test!(
        current_cpu_resched_request_is_one_shot,
        "current cpu resched request is one shot",
        || {
            request_current_cpu_resched();
            assert!(take_current_cpu_resched_request());
            assert!(!take_current_cpu_resched_request());
        }
    );

    crate::test!(
        scheduler_deferred_work_due_deadline,
        "scheduler deferred work detects due deadlines",
        || {
            let work = SchedulerDeferredWork {
                thread_deadline: Some(Time::from_nanoseconds(10)),
                process_timer_deadline: Some(Time::from_nanoseconds(20)),
                timerfd_deadline: None,
                pollers_dirty: false,
                input_pending: false,
            };
            assert!(work.has_due_deadline(Time::from_nanoseconds(10)));
            assert!(!work.has_due_deadline(Time::from_nanoseconds(9)));
        }
    );

    crate::test!(
        scheduler_deferred_work_future_deadline_does_not_block_idle,
        "scheduler deferred work ignores future deadlines for idle gating",
        || {
            let work = SchedulerDeferredWork {
                thread_deadline: Some(Time::from_nanoseconds(10)),
                process_timer_deadline: None,
                timerfd_deadline: None,
                pollers_dirty: false,
                input_pending: false,
            };

            assert!(!work.should_keep_cpu_awake(Time::from_nanoseconds(9)));
            assert!(work.should_keep_cpu_awake(Time::from_nanoseconds(10)));
        }
    );

    crate::test!(
        scheduler_deferred_work_background_reasons_keep_cpu_awake,
        "scheduler deferred work still spins for pollers and input",
        || {
            let dirty_pollers = SchedulerDeferredWork {
                thread_deadline: Some(Time::from_nanoseconds(100)),
                process_timer_deadline: None,
                timerfd_deadline: None,
                pollers_dirty: true,
                input_pending: false,
            };
            let pending_input = SchedulerDeferredWork {
                thread_deadline: None,
                process_timer_deadline: None,
                timerfd_deadline: None,
                pollers_dirty: false,
                input_pending: true,
            };

            assert!(dirty_pollers.should_keep_cpu_awake(Time::from_nanoseconds(1)));
            assert!(pending_input.should_keep_cpu_awake(Time::from_nanoseconds(1)));
        }
    );
}
