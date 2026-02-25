use crate::systemcall::{error::SyscallError, syscall_no::SyscallNo};

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

#[macro_export]
macro_rules! new_syscall {
    (
        $name:ident,
        $num:expr,
        $arg1:ident: $type1:ty,
        $arg2:ident: $type2:ty,
        $arg3:ident: $type3:ty,
        $handler: expr
    ) => {
        #[derive(Clone, Copy)]
        pub struct $name;

        impl SyscallImpl for $name {
            const ENTRY: SyscallNo = $num;

            // depreacted
            fn handle_call(
                arg1: u64,
                arg2: u64,
                arg3: u64,
                _arg4: u64,
                _arg5: u64,
                _arg6: u64,
            ) -> Result<usize, SyscallError> {
                // 宏会自动生成转换代码
                let $arg1 = arg1 as $type1;
                let $arg2 = arg2 as $type2;
                let $arg3 = arg3 as $type3;

                // 调用真正的实现函数
                $handler($arg1, $arg2, $arg3)
            }
        }
    };
}
