use alloc::sync::Arc;

use crate::{
    misc::profile::{self, HotSyscallPhase},
    object::{Object, misc::ObjectRef},
    polling::{PollerObject, event::PollableEvent},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

pub(crate) fn wait_for_object_event(object: ObjectRef, event: PollableEvent) {
    let total_start = profile::scope_start();
    let poller = PollerObject::new();
    let register_start = profile::scope_start();
    poller.register_obj(object, event, 0);
    profile::record_hot_syscall_phase(
        HotSyscallPhase::ReadUnixWaitRegister,
        profile::scope_start().saturating_sub(register_start),
    );

    let fastpath_start = profile::scope_start();
    poller.push_already_ready_events();
    if poller.has_woken_events() {
        let _ = poller.take_woken_events(1);
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadUnixWaitFastpath,
            profile::scope_start().saturating_sub(fastpath_start),
        );
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadUnixWaitReadable,
            profile::scope_start().saturating_sub(total_start),
        );
        return;
    }
    profile::record_hot_syscall_phase(
        HotSyscallPhase::ReadUnixWaitFastpath,
        profile::scope_start().saturating_sub(fastpath_start),
    );

    let poller_ref: Arc<dyn Object> = poller.clone();
    let prepare_start = profile::scope_start();
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline: None,
    });
    profile::record_hot_syscall_phase(
        HotSyscallPhase::ReadUnixWaitPrepareBlock,
        profile::scope_start().saturating_sub(prepare_start),
    );

    let recheck_start = profile::scope_start();
    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }
    profile::record_hot_syscall_phase(
        HotSyscallPhase::ReadUnixWaitRecheck,
        profile::scope_start().saturating_sub(recheck_start),
    );

    let blocked_start = profile::scope_start();
    if poller.has_woken_events() {
        cancel_block(&current);
    } else {
        finish_block_current();
    }
    let blocked_cycles = profile::scope_start().saturating_sub(blocked_start);

    let _ = poller.take_woken_events(1);
    let total_cycles = profile::scope_start().saturating_sub(total_start);
    profile::record_hot_syscall_phase(
        HotSyscallPhase::ReadUnixWaitReadable,
        total_cycles.saturating_sub(blocked_cycles),
    );
}
