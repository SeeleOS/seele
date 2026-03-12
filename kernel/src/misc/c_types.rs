use core::ffi::CStr;

use alloc::{string::String, vec::Vec};
use fatfs::SeekFrom;
use x86_64::registers::segmentation::CS;

use crate::misc::{error::KernelError, others::KernelFrom};

pub type CString = *const u8;
pub type CVec<T> = *const T;

impl KernelFrom<CString> for String {
    fn k_from(val: CString) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        let mut str = String::new();

        for i in 0..MAX_LENGTH {
            unsafe {
                let char = *val.add(i) as char;

                if char == '\0' {
                    return Ok(str);
                }
                str.push(char);
            }
        }

        Err(KernelError::InvalidString)
    }
}

impl KernelFrom<CVec<CString>> for Vec<String> {
    fn k_from(val: CVec<CString>) -> super::error::KernelResult<Self> {
        const MAX_LENGTH: usize = 4096;

        let mut vec = Vec::new();

        for i in 0..MAX_LENGTH {
            unsafe {
                let ptr = *val.add(i);

                if ptr.is_null() {
                    break;
                }
                vec.push(String::k_from(ptr)?);
            }
        }

        Ok(vec)
    }
}
