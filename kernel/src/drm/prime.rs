use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{PhysFrame, Size4KiB},
};

use crate::{
    filesystem::{
        info::{FileLikeInfo, LinuxStat, UnixPermission},
        vfs_traits::FileLikeType,
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::{addrspace::mem_area::Data, protection::Protection, user_safe},
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult, get_object_current_process},
        open_state::OpenState,
        traits::{MemoryMappable, Statable},
    },
    process::{FdFlags, manager::get_current_process, misc::with_current_process},
};

use super::{
    client::DrmPrimeHandle,
    object::DRM_STATE,
    state::DumbBuffer,
    user::{current_debug_process, read_user},
};

static NEXT_PRIME_INODE: AtomicU64 = AtomicU64::new(1);

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DrmPrimeHandleFlags: u32 {
        const CLOEXEC = 0x0008_0000;
        const RDWR = 0x0000_0002;
    }
}

#[derive(Debug)]
pub struct DrmPrimeBufferObject {
    buffer: DumbBuffer,
    inode: u64,
    open_state: OpenState,
}

impl DrmPrimeBufferObject {
    fn new(buffer: DumbBuffer) -> Self {
        Self {
            buffer,
            inode: NEXT_PRIME_INODE.fetch_add(1, Ordering::Relaxed),
            open_state: OpenState::default(),
        }
    }

    fn exported_buffer(&self) -> &DumbBuffer {
        &self.buffer
    }
}

impl Object for DrmPrimeBufferObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function!("mappable", MemoryMappable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("drm_prime_buffer", DrmPrimeBufferObject);
}

impl MemoryMappable for DrmPrimeBufferObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<VirtAddr> {
        if pages == 0 || !offset.is_multiple_of(4096) {
            return Err(ObjectError::InvalidArguments);
        }

        let byte_len = pages
            .checked_mul(4096)
            .ok_or(ObjectError::InvalidArguments)?;
        let end = offset
            .checked_add(byte_len)
            .ok_or(ObjectError::InvalidArguments)?;
        if end > self.buffer.aligned_size() {
            return Err(ObjectError::InvalidArguments);
        }

        let page_delta =
            usize::try_from(offset / 4096).map_err(|_| ObjectError::InvalidArguments)?;
        let page_count = usize::try_from(pages).map_err(|_| ObjectError::InvalidArguments)?;
        let mut frames = Vec::with_capacity(page_count);
        for index in 0..page_count {
            let page_addr = self.buffer.start_frame.start_address().as_u64()
                + ((page_delta + index) as u64 * 4096);
            frames.push(PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(
                page_addr,
            )));
        }

        Ok(with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                protection,
                Data::Shared {
                    frames: Arc::<[PhysFrame<Size4KiB>]>::from(frames),
                    flags: self.buffer.shared_flags,
                },
            )
        }))
    }
}

impl Statable for DrmPrimeBufferObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::new(
            FileLikeInfo::new(
                String::from("drm-prime"),
                usize::try_from(self.buffer.size).unwrap_or(usize::MAX),
                UnixPermission(0o600),
                FileLikeType::File,
            )
            .with_inode(self.inode),
        )
    }
}

pub(super) fn handle_prime_handle_to_fd(ptr: *mut DrmPrimeHandle) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    let flags = DrmPrimeHandleFlags::from_bits(request.flags).ok_or_else(|| {
        crate::s_println!(
            "unsupported drm prime handle flags raw={:#x}",
            request.flags
        );
        ObjectError::InvalidArguments
    })?;
    let buffer = DRM_STATE.lock().get_user_handle(request.handle)?.clone();
    let object: ObjectRef = Arc::new(DrmPrimeBufferObject::new(buffer));
    let fd_flags = if flags.contains(DrmPrimeHandleFlags::CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);
    request.fd = i32::try_from(fd).map_err(|_| ObjectError::Other)?;
    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime_handle_to_fd comm={} pid={} handle={} flags={:#x} fd={}",
            comm,
            pid,
            request.handle,
            request.flags,
            request.fd
        );
    }
    user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_prime_fd_to_handle(ptr: *mut DrmPrimeHandle) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    if request.flags != 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let object =
        get_object_current_process(request.fd as u64).map_err(|_| ObjectError::InvalidArguments)?;
    let prime = object
        .as_drm_prime_buffer()
        .map_err(|_| ObjectError::InvalidArguments)?;
    request.handle = DRM_STATE
        .lock()
        .import_prime_buffer(prime.exported_buffer())?;
    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime_fd_to_handle comm={} pid={} fd={} handle={}",
            comm,
            pid,
            request.fd,
            request.handle
        );
    }
    user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}
