use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use spin::Mutex;

use crate::{
    multitasking::{
        MANAGER, process::manager::block_current, process::process::ProcessID, yielding::BlockType,
    },
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub enum FutexResultType {
    Success = 1,
    TryAgain,
    Invalid,
}

static FUTEX_QUEUE: Mutex<BTreeMap<u64, VecDeque<ProcessID>>> = Mutex::new(BTreeMap::new());

pub struct FutexWakeImpl;
pub struct FutexWaitImpl;

impl SyscallImpl for FutexWaitImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::FutexWait;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let mut queue = FUTEX_QUEUE.lock();
        let cur_value = unsafe { *(arg1 as *mut u64) };

        if cur_value != arg2 {
            return Ok(FutexResultType::TryAgain as usize);
        }

        if !queue.contains_key(&arg1) {
            queue.insert(arg1, VecDeque::new());
        }

        queue
            .get_mut(&arg1)
            .unwrap()
            .push_back(MANAGER.lock().current.unwrap());

        drop(queue);

        //block_current(BlockType::Futex);
        Ok(FutexResultType::Success as usize)
    }
}

impl SyscallImpl for FutexWakeImpl {
    const ENTRY: SyscallNo = SyscallNo::FutexWake;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let mut queue = FUTEX_QUEUE.lock();
        let mut woken = 0;

        if let Some(queue) = queue.get_mut(&arg1) {
            for _ in 0..arg2 {
                if let Some(process) = queue.pop_front() {
                    MANAGER.lock().wake(process);
                    woken += 1;
                } else {
                    break;
                }
            }
        }

        Ok(woken)
    }
}
