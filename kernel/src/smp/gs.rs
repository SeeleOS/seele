#[derive(Debug)]
#[repr(C)]
pub struct GsContext {
    pub kernel_stack_top: u64,
    pub user_stack_top: u64,
    pub cpu_context: *mut core::ffi::c_void,
    pub active_user_extended_state: *mut u8,
    pub active_user_extended_state_saved: u64,
    pub extended_state_uses_xsave: u64,
    pub extended_state_xcr0: u64,
}
