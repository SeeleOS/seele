use x86_rtc::Rtc;

use crate::define_syscall;
use crate::systemcall::error::SyscallError;
use crate::systemcall::utils::SyscallImpl;

define_syscall!(GetTime, { Ok(Rtc::new().get_unix_timestamp() as usize) });
