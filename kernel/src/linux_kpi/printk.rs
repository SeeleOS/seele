use core::{ffi::CStr, ffi::c_char, fmt};

fn c_message(message: *const c_char) -> Option<&'static str> {
    if message.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(message) }.to_str().ok()
}

pub fn linux_print(args: fmt::Arguments) {
    crate::s_println!("{args}");
}

pub fn linux_print_cstr(prefix: &str, message: *const c_char) -> i32 {
    match c_message(message) {
        Some(message) => linux_print(format_args!("{prefix}{message}")),
        None => linux_print(format_args!("{prefix}<invalid c string>")),
    }
    0
}

#[macro_export]
macro_rules! linux_printk {
    ($($arg:tt)*) => {
        $crate::linux_kpi::linux_print(format_args!($($arg)*))
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn printk(message: *const c_char) -> i32 {
    linux_print_cstr("", message)
}

#[unsafe(no_mangle)]
pub extern "C" fn pr_info(message: *const c_char) -> i32 {
    linux_print_cstr("info: ", message)
}

#[unsafe(no_mangle)]
pub extern "C" fn pr_warn(message: *const c_char) -> i32 {
    linux_print_cstr("warn: ", message)
}

#[unsafe(no_mangle)]
pub extern "C" fn pr_err(message: *const c_char) -> i32 {
    linux_print_cstr("error: ", message)
}

#[cfg(test)]
mod tests {
    use core::ffi::c_char;

    use super::*;

    crate::test!(
        linux_kpi_print_symbols,
        "linux kpi print symbols accept c strings and null pointers",
        linux_kpi_print_symbols_accept_c_strings_and_null_pointers
    );

    fn linux_kpi_print_symbols_accept_c_strings_and_null_pointers() {
        static MESSAGE: &[u8] = b"linux kpi test\0";
        assert_eq!(printk(MESSAGE.as_ptr().cast::<c_char>()), 0);
        assert_eq!(pr_info(MESSAGE.as_ptr().cast::<c_char>()), 0);
        assert_eq!(pr_warn(MESSAGE.as_ptr().cast::<c_char>()), 0);
        assert_eq!(pr_err(MESSAGE.as_ptr().cast::<c_char>()), 0);
        assert_eq!(printk(core::ptr::null()), 0);
    }
}
