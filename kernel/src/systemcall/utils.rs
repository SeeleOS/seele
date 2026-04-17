pub use seele_sys::SyscallResult;
pub use seele_sys::errors::SyscallError;

#[macro_export]
macro_rules! register_syscalls {
    // 注意这里的 $( ... ),* 模式
    ($table: expr, $($no: ident),*) => {
        $(
            $table[seele_sys::numbers::SyscallNumber::$no as usize] = Some(
                <$no as SyscallImpl>::handle_call
                    as fn(u64, u64, u64, u64, u64, u64) -> $crate::systemcall::utils::SyscallResult,
            );
        )*
    };
}

#[macro_export]
macro_rules! define_syscall {
    ($name:ident, |$($arg_name:ident : $arg_type:ty),*| $body:block) => {
        pub struct $name;

        impl SyscallImpl for $name {
            fn handle_call(
                arg1: u64, arg2: u64, arg3: u64,
                arg4: u64, arg5: u64, arg6: u64,
            ) -> $crate::systemcall::utils::SyscallResult {
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
            fn handle_call(
                _arg1: u64, _arg2: u64, _arg3: u64,
                _arg4: u64, _arg5: u64, _arg6: u64,
            ) -> $crate::systemcall::utils::SyscallResult {
                $body
            }
        }
    };
}

pub trait SyscallImpl {
    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> SyscallResult;
}
