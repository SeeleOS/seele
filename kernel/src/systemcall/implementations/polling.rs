use crate::systemcall::numbers::*;
use crate::systemcall::utils::SyscallImpl;
use crate::systemcall::{error::SyscallError, implementations::objects};
use alloc::sync::Arc;

use crate::{
    define_syscall, multitasking::process::manager::get_current_process,
    polling::poller::PollerObject,
};

define_syscall!(CreatePoller, {
    let process = get_current_process();
    let objects = &mut process.lock().objects;

    objects.push(Some(Arc::new(PollerObject::new())));

    Ok(objects.len() - 1)
});
