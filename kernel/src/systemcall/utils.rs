use alloc::string::String;

use crate::{
    filesystem::info::LinuxStat,
    misc::others::from_cstr,
    systemcall::{error::SyscallError, syscall_no::SyscallNo},
};

#[macro_export]
macro_rules! register_syscalls {
    // 注意这里的 $( ... ),* 模式
    ($table: expr, $($no: ident),*) => {
        $(
            $table[SyscallNo::$no as usize] = Some(
                <$no as SyscallImpl>::handle_call
                    as fn(u64, u64, u64, u64, u64, u64) -> Result<usize, SyscallError>,
            );
        )*
    };
}

#[macro_export]
macro_rules! define_syscall {
    ($name:ident, |$($arg_name:ident : $arg_type:ty),*| $body:block) => {
        pub struct $name;

        impl SyscallImpl for $name {
            const ENTRY: SyscallNo = SyscallNo::$name;

            fn handle_call(
                arg1: u64, arg2: u64, arg3: u64,
                arg4: u64, arg5: u64, arg6: u64,
            ) -> Result<usize, SyscallError> {
                let args = [arg1, arg2, arg3, arg4, arg5, arg6];
                let mut _idx = 0;

                $(
                    // Type converting
                    let $arg_name: $arg_type = <$arg_type as $crate::systemcall::arg_types::SyscallArg>::from_u64(args[_idx])?;
                    #[allow(unused_assignments)]
                    { _idx += 1; }
                )*

                $body
            }
        }
    };

    ($name:ident, $body:block) => {
        pub struct $name;

        impl SyscallImpl for $name {
            const ENTRY: SyscallNo = SyscallNo::$name;

            fn handle_call(
                _arg1: u64, _arg2: u64, _arg3: u64,
                _arg4: u64, _arg5: u64, _arg6: u64,
            ) -> Result<usize, SyscallError> {
                $body
            }
        }
    };
}

pub trait SyscallImpl {
    const ENTRY: SyscallNo;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, SyscallError>;
}
