use crate::systemcall::error::SyscallError;
use crate::systemcall::numbers::*;
use crate::systemcall::utils::SyscallImpl;
use alloc::sync::Arc;

use crate::{
    define_syscall, multitasking::process::manager::get_current_process,
    polling::poller::PollerObject,
};

define_syscall!(CreatePoller, {
    get_current_process()
        .lock()
        .objects
        .push(Some(Arc::new(PollerObject::new())));

    Ok(0)
});
