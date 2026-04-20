use alloc::{string::String, vec::Vec};

use crate::{
    memory::user_safe,
    misc::{error::KernelError, others::KernelFrom},
};

pub type CString = *const u8;
pub type CVec<T> = *const T;

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl KernelFrom<CString> for String {
    fn k_from(val: CString) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        if val.is_null() {
            return Err(KernelError::InvalidString);
        }

        let mut str = String::new();

        for i in 0..MAX_LENGTH {
            let byte =
                user_safe::read(unsafe { val.add(i) }).map_err(|_| KernelError::InvalidString)?;
            let char = byte as char;

            if char == '\0' {
                return Ok(str);
            }
            str.push(char);
        }

        Err(KernelError::InvalidString)
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl KernelFrom<CVec<CString>> for Vec<String> {
    fn k_from(val: CVec<CString>) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        if val.is_null() {
            return Ok(Vec::new());
        }

        let mut vec = Vec::new();

        for i in 0..MAX_LENGTH {
            let ptr =
                user_safe::read(unsafe { val.add(i) }).map_err(|_| KernelError::InvalidString)?;

            if ptr.is_null() {
                break;
            }
            vec.push(String::k_from(ptr)?);
        }

        Ok(vec)
    }
}
