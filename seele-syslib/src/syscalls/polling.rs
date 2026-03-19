use crate::{syscall, utils::SyscallResult};

pub fn create_poller() -> SyscallResult {
    syscall!(CreatePoller)
}
