use crate::{
    misc::snapshot::Snapshot,
    multitasking::{
        MANAGER,
        thread::{
            THREAD_MANAGER,
            manager::ThreadManager,
            misc::State,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
        },
    },
    s_println,
    tss::TSS,
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
        let exec_rsp = (*executor_snapshot).kernel_rsp;
        let exec_ret = *(exec_rsp as *const u64);
        s_println!(
            "ret_exec type {:?} save rip {:#x} rsp {:#x} exec_rsp {:#x} exec_ret {:#x}",
            snapshot_type,
            snapshot.rip,
            snapshot.rsp,
            exec_rsp,
            exec_ret
        );
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

    s_println!("block save rip {:#x} rsp {:#x}", ret_addr, ret_rsp);
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
