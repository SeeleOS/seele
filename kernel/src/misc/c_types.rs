use alloc::{string::String, vec::Vec};

use crate::misc::error::KernelError;

pub type CStringPtr = *const u8;

/// # Safety
/// Caller mush provide valid pointer
pub unsafe fn from_cstr(ptr: CStringPtr) -> Result<String, KernelError> {
    const MAX_LENGTH: usize = 4096;

    let mut str = String::new();

    for i in 0..MAX_LENGTH {
        unsafe {
            let char = *ptr.add(i) as char;

            if char == '\0' {
                return Ok(str);
            }
            str.push(char);
        }
    }

    Err(KernelError::InvalidString)
}

pub fn from_c_array(ptr: *const CStringPtr) -> Result<Vec<String>, KernelError> {
    const MAX_LENGTH: usize = 4096;

    let mut vec = Vec::new();

    for i in 0..MAX_LENGTH {
        unsafe {
            let val = *ptr.sub(i);

            if val.is_null() {
                break;
            }
            vec.push(from_cstr(val)?);
        }
    }

    Ok(vec)
}
