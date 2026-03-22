use crate::{
    misc::snapshot::Snapshot,
    multitasking::thread::{
            THREAD_MANAGER,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
        },
};

pub fn return_to_executor(snapshot: &mut Snapshot, snapshot_type: ThreadSnapshotType) {
    let (thread_snapshot, executor_snapshot) = {
        let manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = manager.current.clone().unwrap();
        let mut current = current_ref.lock();

        (
            &mut current.snapshot as *mut ThreadSnapshot,
            &mut current.executor_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe {
        (*thread_snapshot).snapshot_type = snapshot_type;
        (*executor_snapshot).switch_from(Some(&mut *thread_snapshot), Some(snapshot));
    }
}

#[unsafe(naked)]
pub extern "C" fn return_to_executor_from_current() {
    core::arch::naked_asm!(
        // rdi = return address, rsi = caller's rsp after ret
        "mov rdi, [rsp]",
        "lea rsi, [rsp + 8]",
        "jmp {inner}",
        inner = sym return_to_executor_from_current_inner,
    )
}

extern "C" fn return_to_executor_from_current_inner(ret_addr: u64, ret_rsp: u64) {
    log::trace!("return_to_executor_from_current");
    let mut snapshot = Snapshot::from_current();

    snapshot.rip = ret_addr;
    snapshot.rsp = ret_rsp;

    return_to_executor(&mut snapshot, ThreadSnapshotType::Kernel);
}

pub fn return_to_executor_no_save() {
    log::trace!("return_to_executor_no_save");
    let (thread_snapshot, executor_snapshot) = {
        let manager = THREAD_MANAGER.get().unwrap().lock();
        let current_ref = manager.current.clone().unwrap();
        let mut current = current_ref.lock();

        (
            &mut current.snapshot as *mut ThreadSnapshot,
            &mut current.executor_snapshot as *mut ThreadSnapshot,
        )
    };

    unsafe { (*executor_snapshot).switch_from(Some(&mut *thread_snapshot), None) };
}
