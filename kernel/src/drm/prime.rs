use core::sync::atomic::{AtomicU64, Ordering};

use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc};
use bitflags::bitflags;
use x86_64::{
    VirtAddr,
    structures::paging::{PhysFrame, Size4KiB},
};

use crate::{
    filesystem::{
        info::{FileLikeInfo, LinuxStat, UnixPermission},
        vfs_traits::{FileLikeType, Whence},
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::{addrspace::mem_area::Data, protection::Protection, user_safe},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        linux_anon::{EventFdFlags, EventFdObject},
        misc::{ObjectRef, ObjectResult, get_object_current_process},
        open_state::OpenState,
        traits::{Configuratable, MemoryMappable, Seekable, Statable},
    },
    process::{FdFlags, manager::get_current_process, misc::with_current_process},
};

use super::{client::DrmPrimeHandle, object::DRM_STATE, state::DumbBuffer, user::read_user};

static NEXT_PRIME_INODE: AtomicU64 = AtomicU64::new(1);
bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DrmPrimeHandleFlags: u32 {
        const CLOEXEC = 0x0008_0000;
        const RDWR = 0x0000_0002;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DmaBufSync {
    pub flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DmaBufExportSyncFile {
    pub flags: u32,
    pub fd: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DmaBufImportSyncFile {
    pub flags: u32,
    pub fd: i32,
}

#[derive(Debug)]
pub struct DrmPrimeBufferObject {
    buffer: DumbBuffer,
    inode: u64,
    open_state: OpenState,
    position: Mut<usize>,
}

impl DrmPrimeBufferObject {
    fn new(buffer: DumbBuffer) -> Self {
        Self {
            buffer,
            inode: NEXT_PRIME_INODE.fetch_add(1, Ordering::Relaxed),
            open_state: OpenState::default(),
            position: Mut::new(0),
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

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("mappable", MemoryMappable);
    impl_cast_function!("seekable", Seekable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("drm_prime_buffer", DrmPrimeBufferObject);
}

impl Configuratable for DrmPrimeBufferObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::DmaBufSync(ptr) => handle_dmabuf_sync_ioctl(ptr),
            ConfigurateRequest::DmaBufExportSyncFile(ptr) => {
                handle_dmabuf_export_sync_file_ioctl(ptr)
            }
            ConfigurateRequest::DmaBufImportSyncFile(ptr) => {
                handle_dmabuf_import_sync_file_ioctl(ptr)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
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
        let frames = Arc::<[PhysFrame<Size4KiB>]>::from(
            self.buffer.frames[page_delta..page_delta + page_count].to_vec(),
        );

        Ok(with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                protection,
                Data::Shared {
                    frames,
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

impl Seekable for DrmPrimeBufferObject {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize> {
        let len = i64::try_from(self.buffer.aligned_size()).map_err(|_| ObjectError::Other)?;
        let mut position = self.position.lock();
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => (*position as i64)
                .checked_add(offset)
                .ok_or(ObjectError::Other)?,
            Whence::End => len.checked_add(offset).ok_or(ObjectError::Other)?,
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(ObjectError::InvalidArguments);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(ObjectError::InvalidArguments);
                }
                len
            }
        };
        if next < 0 {
            return Err(ObjectError::InvalidArguments);
        }
        *position = next as usize;
        Ok(*position)
    }
}

pub(super) fn handle_prime_handle_to_fd(ptr: *mut DrmPrimeHandle) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    let flags =
        DrmPrimeHandleFlags::from_bits(request.flags).ok_or(ObjectError::InvalidArguments)?;
    let buffer = DRM_STATE.lock().get_user_handle(request.handle)?.clone();
    let object: ObjectRef = Arc::new(DrmPrimeBufferObject::new(buffer.clone()));
    let fd_flags = if flags.contains(DrmPrimeHandleFlags::CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);
    request.fd = i32::try_from(fd).map_err(|_| ObjectError::Other)?;
    user_safe::write(ptr, &request).map_err(|_| ObjectError::BadAddress)?;
    Ok(0)
}

fn handle_dmabuf_sync_ioctl(ptr: *mut DmaBufSync) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u64 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u64 = 2;
    const DMA_BUF_SYNC_END: u64 = 1 << 2;
    const DMA_BUF_SYNC_VALID_FLAGS_MASK: u64 =
        DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE | DMA_BUF_SYNC_END;

    let sync = read_user(ptr)?;
    if sync.flags & !DMA_BUF_SYNC_VALID_FLAGS_MASK != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if sync.flags & (DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE) == 0 {
        return Err(ObjectError::InvalidArguments);
    }
    Ok(0)
}

fn handle_dmabuf_export_sync_file_ioctl(ptr: *mut DmaBufExportSyncFile) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u32 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u32 = 2;

    let mut sync_file = read_user(ptr)?;
    if sync_file.flags & !(DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE) != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if sync_file.flags == 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let fd = get_current_process().lock().push_object_with_flags(
        EventFdObject::new(1, EventFdFlags::empty()),
        FdFlags::empty(),
    );
    sync_file.fd = i32::try_from(fd).map_err(|_| ObjectError::Other)?;
    user_safe::write(ptr, &sync_file).map_err(|_| ObjectError::BadAddress)?;

    Ok(0)
}

fn handle_dmabuf_import_sync_file_ioctl(ptr: *mut DmaBufImportSyncFile) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u32 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u32 = 2;

    let sync_file = read_user(ptr)?;
    if sync_file.flags & !(DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE) != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if sync_file.flags == 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let _ =
        get_object_current_process(sync_file.fd as u64).map_err(|_| ObjectError::DoesNotExist)?;

    Ok(0)
}

pub(super) fn handle_prime_fd_to_handle(ptr: *mut DrmPrimeHandle) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    if request.flags != 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let object =
        get_object_current_process(request.fd as u64).map_err(|_| ObjectError::DoesNotExist)?;
    let prime = object
        .as_drm_prime_buffer()
        .map_err(|_| ObjectError::InvalidArguments)?;
    let exported = prime.exported_buffer();
    request.handle = DRM_STATE.lock().import_prime_buffer(exported)?;
    user_safe::write(ptr, &request).map_err(|_| ObjectError::BadAddress)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::DrmPrimeHandleFlags;
    use crate::object::{config::ConfigurateRequest, linux_ioctl::ioctl_request};

    crate::test!(
        drm_prime_dmabuf_ioctl_decode,
        "drm prime dmabuf ioctl decode routes dmabuf requests to typed configurate requests",
        drm_prime_dmabuf_ioctl_decode_routes_dmabuf_requests_to_typed_configurate_requests
    );
    crate::test!(
        drm_prime_ioctl_name_fallbacks,
        "drm prime dmabuf ioctl decode leaves non-dmabuf and unknown requests raw",
        drm_prime_dmabuf_ioctl_decode_leaves_non_dmabuf_and_unknown_requests_raw
    );
    crate::test!(
        drm_prime_handle_flag_bits,
        "drm prime handle flags expose linux rdwr and cloexec bits",
        drm_prime_handle_flags_expose_linux_rdwr_and_cloexec_bits
    );

    fn drm_prime_dmabuf_ioctl_decode_routes_dmabuf_requests_to_typed_configurate_requests() {
        assert!(matches!(
            ConfigurateRequest::new(ioctl_request(0, b'b', 0, 8), 0x1000).unwrap(),
            ConfigurateRequest::DmaBufSync(ptr) if ptr as usize == 0x1000
        ));
        assert!(matches!(
            ConfigurateRequest::new(ioctl_request(0, b'b', 2, 8), 0x2000).unwrap(),
            ConfigurateRequest::DmaBufExportSyncFile(ptr) if ptr as usize == 0x2000
        ));
        assert!(matches!(
            ConfigurateRequest::new(ioctl_request(0, b'b', 3, 8), 0x3000).unwrap(),
            ConfigurateRequest::DmaBufImportSyncFile(ptr) if ptr as usize == 0x3000
        ));
    }

    fn drm_prime_dmabuf_ioctl_decode_leaves_non_dmabuf_and_unknown_requests_raw() {
        let non_dmabuf = ioctl_request(0, b'x', 0, 0);
        let unknown_dmabuf = ioctl_request(0, b'b', 9, 0);

        assert!(matches!(
            ConfigurateRequest::new(non_dmabuf, 0x4000).unwrap(),
            ConfigurateRequest::RawIoctl { request, arg }
                if request == non_dmabuf && arg == 0x4000
        ));
        assert!(matches!(
            ConfigurateRequest::new(unknown_dmabuf, 0x5000).unwrap(),
            ConfigurateRequest::RawIoctl { request, arg }
                if request == unknown_dmabuf && arg == 0x5000
        ));
    }

    fn drm_prime_handle_flags_expose_linux_rdwr_and_cloexec_bits() {
        let flags = DrmPrimeHandleFlags::RDWR | DrmPrimeHandleFlags::CLOEXEC;
        assert!(flags.contains(DrmPrimeHandleFlags::RDWR));
        assert!(flags.contains(DrmPrimeHandleFlags::CLOEXEC));
        assert_eq!(DrmPrimeHandleFlags::RDWR.bits(), 0x0000_0002);
        assert_eq!(DrmPrimeHandleFlags::CLOEXEC.bits(), 0x0008_0000);
    }
}
