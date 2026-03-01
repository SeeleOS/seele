#[macro_export]
macro_rules! wrap_c {
    ($function:ident (
        $($normal_arg:ident : $normal_typ:ty),* $(, $slice_arg:ident : &[u8])* $(, $mut_slice_arg:ident : &mut [u8])*
    )) => {
        paste::paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [<c_sys_$function>](
                $($normal_arg : $normal_typ,)*
                $($slice_arg [<_ptr>]: *const u8, $slice_arg [<_len>]: usize,)*
                $($mut_slice_arg [<_ptr>]: *mut u8, $mut_slice_arg [<_len>]: usize)*
            ) -> isize {
                // 在内部把 C 的 ptr/len 组装回 Rust 的切片
                $(
                    let $slice_arg = unsafe { core::slice::from_raw_parts($slice_arg [<_ptr>], $slice_arg [<_len>]) };
                )*
                $(
                    let $mut_slice_arg = unsafe { core::slice::from_raw_parts_mut($mut_slice_arg [<_ptr>], $mut_slice_arg [<_len>]) };
                )*

                // 执行原函数
                match $function($($normal_arg,)* $($slice_arg,)* $($mut_slice_arg,)*) {
                    Ok(val) => val as isize,
                    Err(val) => val.as_isize(),
                }
            }
        }
    };
}
