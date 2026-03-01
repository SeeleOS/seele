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
