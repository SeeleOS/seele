use alloc::alloc::{Layout, alloc_zeroed, dealloc};
use core::ffi::c_void;

pub(super) unsafe extern "C" fn flanterm_alloc(size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size.max(1), 16).unwrap();
    unsafe { alloc_zeroed(layout).cast() }
}

pub(super) unsafe extern "C" fn flanterm_free(ptr: *mut c_void, size: usize) {
    if ptr.is_null() {
        return;
    }

    let layout = Layout::from_size_align(size.max(1), 16).unwrap();
    unsafe { dealloc(ptr.cast(), layout) };
}
