#[macro_export]
macro_rules! register_syscall {
    ($table: expr, $no: expr, $val: ty) => {
        $table[$no as usize] = Some(
            <$val as SyscallImpl>::handle_call
                as fn(u64, u64, u64, u64, u64, u64) -> Result<usize, SyscallError>,
        );
    };
}

#[macro_export]
macro_rules! define_syscall {
    ($name:ident, |$($arg_name:ident : $arg_type:ty),*| $body:block) => {
        paste::paste! {
        pub struct [<$name Impl>];

        impl SyscallImpl for $name {
            const ENTRY: SyscallNo = SyscallNo::$name;

            fn handle_call(
                arg1: u64, arg2: u64, arg3: u64,
                arg4: u64, arg5: u64, arg6: u64,
            ) -> Result<usize, SyscallError> {
                let args = [arg1, arg2, arg3, arg4, arg5, arg6];
                let mut idx = 0;

                // Cast types
                $(
                    let $arg_name = match stringify!($arg_type) {
                        "&str" => unsafe { from_cstr(args[idx] as *const u8)? },
                        "i32"  => args[idx] as i32,
                        "u32"  => args[idx] as u32,
                        "usize" => args[idx] as usize,
                        "bool" => args[idx] as bool,
                        "*mut LinuxStat" => args[idx] as *mut LinuxStat,
                        _ => args[idx] as $arg_type, // 默认强转
                    };
                    #[allow(unused_assignments)]
                    { idx += 1; }
                )*

                $body
            }
        }
        }
    };
}
