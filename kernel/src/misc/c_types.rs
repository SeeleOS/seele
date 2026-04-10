use alloc::{string::String, vec::Vec};

use crate::misc::{error::KernelError, others::KernelFrom, usercopy::read_user_c_string, usercopy::read_user_value};

pub type CString = *const u8;
pub type CVec<T> = *const T;

impl KernelFrom<CString> for String {
    fn k_from(val: CString) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        let bytes = read_user_c_string(val, MAX_LENGTH).ok_or(KernelError::InvalidString)?;
        String::from_utf8(bytes).map_err(|_| KernelError::InvalidString)
    }
}

impl KernelFrom<CVec<CString>> for Vec<String> {
    fn k_from(val: CVec<CString>) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        if val.is_null() {
            return Ok(Vec::new());
        }

        let mut vec = Vec::new();

        for i in 0..MAX_LENGTH {
            let ptr = read_user_value(unsafe { val.add(i) }).ok_or(KernelError::InvalidString)?;

            if ptr.is_null() {
                break;
            }
            vec.push(String::k_from(ptr)?);
        }

        Ok(vec)
    }
}
