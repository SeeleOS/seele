#[macro_export]
macro_rules! wrap_c {
    ($function: ident ($($arg:ident : $typ:ty),*)) => {
        paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<c_sys_$function>]($($arg: $typ),*) -> isize {
                // Execute the inner syscall
                match $function($($arg),*) {
                    // Just returns the return value if success
                    Ok(val) => val as isize,
                    // Translate the error to error code and return
                    Err(val) => val.as_isize(),
                }
            }
        }
    };
}
#[macro_export]
macro_rules! wrap_c_fat_pointer {
    // 匹配：函数名(普通参数, 切片参数: &mut [u8])
    ($function:ident ($($normal_arg:ident : $normal_typ:ty),* ; $slice_arg:ident : &mut [u8])) => {
        paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<c_sys_$function>]($($normal_arg : $normal_typ,)* ptr: *mut u8, len: usize) -> isize {
                let buffer = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
                match $function($($normal_arg,)* buffer) {
                    Ok(val) => val as isize,
                    Err(val) => val.as_isize(),
                }
            }
        }
    };

    // 匹配：函数名(普通参数, 切片参数: &[u8])
    ($function:ident ($($normal_arg:ident : $normal_typ:ty),* ; $slice_arg:ident : &[u8])) => {
        paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<c_sys_$function>]($($normal_arg : $normal_typ,)* ptr: *const u8, len: usize) -> isize {
                let buffer = unsafe { core::slice::from_raw_parts(ptr, len) };
                match $function($($normal_arg,)* buffer) {
                    Ok(val) => val as isize,
                    Err(val) => val.as_isize(),
                }
            }
        }
    };
}
