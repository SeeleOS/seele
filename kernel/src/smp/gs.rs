#[derive(Debug)]
#[repr(C)]
pub struct GsContext {
    pub kernel_stack_top: u64,
    pub user_stack_top: u64,
    pub cpu_context: *mut core::ffi::c_void,
}
