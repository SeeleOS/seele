use core::{
    alloc::Layout,
    mem::{align_of, size_of},
    ptr::{self, NonNull},
};

use crate::memory::heap;

const GFP_KERNEL: u32 = 0x10;
const GFP_ATOMIC: u32 = 0x20;
const ZERO_SIZE_PTR: usize = 16;
const KMALLOC_MIN_ALIGN: usize = align_of::<u64>();

#[repr(C)]
struct AllocationHeader {
    total_size: usize,
    align: usize,
    offset: usize,
}

fn zero_or_null_ptr(ptr: *mut u8) -> bool {
    ptr.addr() <= ZERO_SIZE_PTR
}

fn kmalloc_align(size: usize) -> usize {
    if size == 0 {
        return KMALLOC_MIN_ALIGN;
    }
    KMALLOC_MIN_ALIGN.max(size & size.wrapping_neg())
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    Some(value.checked_add(align.checked_sub(1)?)? & !(align - 1))
}

fn layout_for(size: usize) -> Option<Layout> {
    let header_size = size_of::<AllocationHeader>();
    let align = kmalloc_align(size).max(align_of::<AllocationHeader>());
    let total_size = header_size
        .checked_add(size.max(1))?
        .checked_add(align.checked_sub(1)?)?;
    Layout::from_size_align(total_size, align).ok()
}

pub fn linux_kmalloc(size: usize, flags: u32) -> *mut u8 {
    let _ = flags & (GFP_KERNEL | GFP_ATOMIC);
    if size == 0 {
        return ZERO_SIZE_PTR as *mut u8;
    }
    let Some(layout) = layout_for(size) else {
        return ptr::null_mut();
    };
    let raw = heap::allocate(layout);
    let Some(raw) = NonNull::<u8>::new(raw) else {
        return ptr::null_mut();
    };
    let Some(data_addr) = align_up(
        unsafe { raw.as_ptr().add(size_of::<AllocationHeader>()) }.addr(),
        layout.align(),
    ) else {
        return ptr::null_mut();
    };
    let data = data_addr as *mut u8;
    let offset = data.addr().saturating_sub(raw.as_ptr().addr());
    unsafe {
        data.sub(size_of::<AllocationHeader>())
            .cast::<AllocationHeader>()
            .write(AllocationHeader {
                total_size: layout.size(),
                align: layout.align(),
                offset,
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
    if zero_or_null_ptr(ptr) {
        return;
    }
    let header = unsafe {
        ptr.sub(size_of::<AllocationHeader>())
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
        linux_kpi_alloc_symbols,
        "linux kpi allocation symbols follow basic kmalloc semantics",
        linux_kpi_allocation_symbols_follow_basic_kmalloc_semantics
    );

    fn linux_kpi_allocation_symbols_follow_basic_kmalloc_semantics() {
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

    crate::test!(
        linux_kpi_alloc_translated_slab_kunit_semantics,
        "linux kpi allocation symbols match translated linux slab kunit semantics",
        linux_kpi_allocation_symbols_match_translated_linux_slab_kunit_semantics
    );

    fn linux_kpi_allocation_symbols_match_translated_linux_slab_kunit_semantics() {
        let zero = linux_kmalloc(0, GFP_KERNEL);
        assert_eq!(zero.addr(), ZERO_SIZE_PTR);
        unsafe {
            linux_kfree(zero);
            linux_kfree(core::ptr::null_mut());
        }

        for size in [8usize, 16, 32, 64] {
            let ptr = linux_kmalloc(size, GFP_KERNEL);
            assert!(!ptr.is_null());
            assert_eq!(ptr.addr() % size, 0);
            unsafe {
                linux_kfree(ptr);
            }
        }

        for size in [24usize, 40, 96] {
            let ptr = linux_kmalloc(size, GFP_KERNEL);
            assert!(!ptr.is_null());
            assert_eq!(ptr.addr() % KMALLOC_MIN_ALIGN, 0);
            unsafe {
                linux_kfree(ptr);
            }
        }

        let zeroed = linux_kcalloc(3, 7, GFP_KERNEL);
        assert!(!zeroed.is_null());
        for index in 0..21 {
            assert_eq!(unsafe { *zeroed.add(index) }, 0);
        }
        unsafe {
            linux_kfree(zeroed);
        }

        assert!(linux_kcalloc(usize::MAX / 2 + 1, 2, GFP_KERNEL).is_null());
    }

    crate::test!(
        linux_kpi_alloc_translated_printf_kunit_buffer,
        "linux kpi allocation supports translated printf kunit guard buffer pattern",
        linux_kpi_allocation_supports_translated_printf_kunit_guard_buffer_pattern
    );

    fn linux_kpi_allocation_supports_translated_printf_kunit_guard_buffer_pattern() {
        const BUF_SIZE: usize = 256;
        const PAD_SIZE: usize = 16;
        const FILL_CHAR: u8 = b'$';

        let alloced_buffer = linux_kmalloc(BUF_SIZE + 2 * PAD_SIZE, GFP_KERNEL);
        assert!(!alloced_buffer.is_null());
        unsafe {
            alloced_buffer.write_bytes(FILL_CHAR, BUF_SIZE + 2 * PAD_SIZE);
        }

        let test_buffer = unsafe { alloced_buffer.add(PAD_SIZE) };
        unsafe {
            test_buffer.write_bytes(0, BUF_SIZE);
        }

        for index in 0..PAD_SIZE {
            assert_eq!(unsafe { *alloced_buffer.add(index) }, FILL_CHAR);
            assert_eq!(
                unsafe { *alloced_buffer.add(PAD_SIZE + BUF_SIZE + index) },
                FILL_CHAR
            );
        }
        for index in 0..BUF_SIZE {
            assert_eq!(unsafe { *test_buffer.add(index) }, 0);
        }

        unsafe {
            linux_kfree(alloced_buffer);
        }
    }
}
