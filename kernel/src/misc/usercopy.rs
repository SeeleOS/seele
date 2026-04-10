use alloc::vec::Vec;
use core::{mem, ptr};
use x86_64::{VirtAddr, structures::paging::PageTableFlags};

use crate::{
    memory::addrspace::KERNEL_MEM_START,
    process::manager::get_current_process,
};

fn validate_user_range_raw(addr: u64, len: usize, write: bool) -> bool {
    if len == 0 {
        return true;
    }

    let Some(last_addr) = addr.checked_add(len as u64 - 1) else {
        return false;
    };

    if addr >= KERNEL_MEM_START || last_addr >= KERNEL_MEM_START {
        return false;
    }

    let process = get_current_process();
    let process = process.lock();
    let mut current = addr;

    while current <= last_addr {
        let virt = VirtAddr::new(current);
        let Some(area) = process.addrspace.get_area(virt) else {
            return false;
        };

        if !area.flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            return false;
        }

        if write && !area.flags.contains(PageTableFlags::WRITABLE) {
            return false;
        }

        let next_page = ((current >> 12) + 1) << 12;
        if next_page == 0 || next_page > last_addr {
            break;
        }
        current = next_page;
    }

    true
}

pub fn validate_user_read(addr: *const u8, len: usize) -> bool {
    validate_user_range_raw(addr as u64, len, false)
}

pub fn validate_user_write(addr: *mut u8, len: usize) -> bool {
    validate_user_range_raw(addr as u64, len, true)
}

pub fn read_user_value<T: Copy>(src: *const T) -> Option<T> {
    if !validate_user_read(src.cast::<u8>(), mem::size_of::<T>()) {
        return None;
    }

    Some(unsafe { ptr::read_unaligned(src) })
}

pub fn write_user_value<T: Copy>(dst: *mut T, value: T) -> bool {
    if !validate_user_write(dst.cast::<u8>(), mem::size_of::<T>()) {
        return false;
    }

    unsafe {
        ptr::write_unaligned(dst, value);
    }
    true
}

pub fn copy_from_user(src: *const u8, dst: &mut [u8]) -> bool {
    if !validate_user_read(src, dst.len()) {
        return false;
    }

    unsafe {
        ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), dst.len());
    }
    true
}

pub fn copy_to_user(dst: *mut u8, src: &[u8]) -> bool {
    if !validate_user_write(dst, src.len()) {
        return false;
    }

    unsafe {
        ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
    }
    true
}

pub fn read_user_c_string(src: *const u8, max_len: usize) -> Option<Vec<u8>> {
    if src.is_null() {
        return None;
    }

    let mut bytes = Vec::new();

    for i in 0..max_len {
        let byte = read_user_value(unsafe { src.add(i) })?;
        if byte == 0 {
            return Some(bytes);
        }
        bytes.push(byte);
    }

    None
}
