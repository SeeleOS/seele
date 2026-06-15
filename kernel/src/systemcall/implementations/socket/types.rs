#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct relibc_iovec {
    pub(super) iov_base: *mut u8,
    pub(super) iov_len: usize,
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct relibc_msg_hdr {
    pub(super) msg_name: *mut u8,
    pub(super) msg_namelen: u32,
    pub(super) msg_iov: *mut relibc_iovec,
    pub(super) msg_iovlen: usize,
    pub(super) msg_control: *mut u8,
    pub(super) msg_controllen: usize,
    pub(super) msg_flags: i32,
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct relibc_mmsghdr {
    pub(super) msg_hdr: relibc_msg_hdr,
    pub(super) msg_len: u32,
}
