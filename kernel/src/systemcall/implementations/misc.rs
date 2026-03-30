use seele_sys::misc::SystemInfo;
use x86_rtc::Rtc;

use crate::misc::time::Time;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
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
