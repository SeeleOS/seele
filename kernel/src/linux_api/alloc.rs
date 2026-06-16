use core::{
    alloc::Layout,
    mem::{align_of, size_of},
    ptr::{self, NonNull},
};

use crate::memory::heap;

const GFP_KERNEL: u32 = 0x10;
const GFP_ATOMIC: u32 = 0x20;

#[repr(C)]
struct AllocationHeader {
    total_size: usize,
    align: usize,
    offset: usize,
}

fn layout_for(size: usize) -> Option<Layout> {
    let header_size = size_of::<AllocationHeader>();
    let align = align_of::<AllocationHeader>();
    let total_size = header_size.checked_add(size.max(1))?;
    Layout::from_size_align(total_size, align).ok()
}

fn allocation_offset() -> usize {
    size_of::<AllocationHeader>()
}

pub fn linux_kmalloc(size: usize, flags: u32) -> *mut u8 {
    let _ = flags & (GFP_KERNEL | GFP_ATOMIC);
    let Some(layout) = layout_for(size) else {
        return ptr::null_mut();
    };
    let raw = heap::allocate(layout);
    let Some(raw) = NonNull::<u8>::new(raw) else {
        return ptr::null_mut();
    };
    let data = unsafe { raw.as_ptr().add(allocation_offset()) };
    unsafe {
        raw.as_ptr()
            .cast::<AllocationHeader>()
            .write(AllocationHeader {
                total_size: layout.size(),
                align: layout.align(),
                offset: allocation_offset(),
            });
    }
    data
}

pub fn linux_kzalloc(size: usize, flags: u32) -> *mut u8 {
    let ptr = linux_kmalloc(size, flags);
    if !ptr.is_null() {
        unsafe {
            ptr.write_bytes(0, size);
        }
    }
    ptr
}

pub fn linux_kcalloc(n: usize, size: usize, flags: u32) -> *mut u8 {
    let Some(total) = n.checked_mul(size) else {
        return ptr::null_mut();
    };
    linux_kzalloc(total, flags)
}

/// # Safety
///
/// `ptr` must be null or a pointer returned by `linux_kmalloc`, `linux_kzalloc`,
/// or `linux_kcalloc` that has not already been freed.
pub unsafe fn linux_kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let header = unsafe {
        ptr.sub(allocation_offset())
            .cast::<AllocationHeader>()
            .read()
    };
    let Some(layout) = Layout::from_size_align(header.total_size, header.align).ok() else {
        return;
    };
    let raw = unsafe { ptr.sub(header.offset) };
    unsafe {
        heap::deallocate(raw, layout);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kmalloc(size: usize, flags: u32) -> *mut u8 {
    linux_kmalloc(size, flags)
}

#[unsafe(no_mangle)]
pub extern "C" fn kzalloc(size: usize, flags: u32) -> *mut u8 {
    linux_kzalloc(size, flags)
}

#[unsafe(no_mangle)]
pub extern "C" fn kcalloc(n: usize, size: usize, flags: u32) -> *mut u8 {
    linux_kcalloc(n, size, flags)
}

/// # Safety
///
/// `ptr` must be null or a pointer returned by `kmalloc`, `kzalloc`, or
/// `kcalloc` that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kfree(ptr: *mut u8) {
    unsafe {
        linux_kfree(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::test!(
        linux_api_alloc_symbols,
        "linux api allocation symbols follow basic kmalloc semantics",
        linux_api_allocation_symbols_follow_basic_kmalloc_semantics
    );

    fn linux_api_allocation_symbols_follow_basic_kmalloc_semantics() {
        let ptr = linux_kmalloc(16, 0);
        assert!(!ptr.is_null());
        unsafe {
            ptr.write_bytes(0xaa, 16);
            linux_kfree(ptr);
        }

        let zeroed = linux_kzalloc(32, 0);
        assert!(!zeroed.is_null());
        for index in 0..32 {
            assert_eq!(unsafe { *zeroed.add(index) }, 0);
        }
        unsafe {
            linux_kfree(zeroed);
        }

        let array = linux_kcalloc(4, 8, 0);
        assert!(!array.is_null());
        for index in 0..32 {
            assert_eq!(unsafe { *array.add(index) }, 0);
        }
        unsafe {
            linux_kfree(array);
        }

        assert!(linux_kcalloc(usize::MAX, 2, 0).is_null());
        unsafe {
            linux_kfree(core::ptr::null_mut());
        }
    }
}
