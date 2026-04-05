use alloc::sync::Arc;
use seele_sys::misc::SystemInfo;
use x86_rtc::Rtc;

use crate::misc::time::Time;
use crate::process::manager::get_current_process;
use crate::process::misc::with_current_process;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::thread::misc::with_current_thread;
use crate::thread::yielding::{BlockType, block_current};
use crate::{NAME, define_syscall};

define_syscall!(GetCurrentTime, {
    Ok(Time::current().as_nanoseconds() as usize)
});

define_syscall!(TimeSinceBoot, {
    Ok(Time::since_boot().as_nanoseconds() as usize)
});

define_syscall!(GetSystemInfo, |info: *mut SystemInfo| {
    unsafe {
        info.write(SystemInfo::new(NAME, env!("CARGO_PKG_VERSION")));
    }

    Ok(0)
});

define_syscall!(Sleep, |nanoseconds: u64| {
    let time = Time::since_boot().add_ns(nanoseconds);

    block_current(BlockType::SetTime(time));

    Ok(0)
});

define_syscall!(ThreadClone, |stack_pointer: u64| {
    with_current_thread(|thread| {
        let process = get_current_process();
        let thread = thread.clone_and_spawn(process.clone());

        thread.lock().snapshot.inner.rsp = stack_pointer;
        thread.lock().snapshot.inner.rax = 0;

        process.clone().lock().threads.push(Arc::downgrade(&thread));

        Ok(thread.lock().id.0 as usize)
    })
});
