use seele_sys::misc::SystemInfo;
use x86_rtc::Rtc;

use crate::systemcall::error::SyscallError;
use crate::systemcall::utils::SyscallImpl;
use crate::{NAME, define_syscall};

define_syscall!(GetTime, { Ok(Rtc::new().get_unix_timestamp() as usize) });

define_syscall!(GetSystemInfo, |info: *mut SystemInfo| {
    unsafe {
        info.write(SystemInfo::new(NAME, env!("CARGO_PKG_VERSION")));
    }

    Ok(0)
});
