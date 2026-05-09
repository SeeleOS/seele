use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{string::String, sync::Arc};
use bitflags::bitflags;
use spin::Mutex;
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
        linux_ioctl::{
            DMABUF_IOCTL_TYPE, drm_prime_raw_ioctl_op, ioctl_nr, ioctl_size, ioctl_type,
        },
        misc::{ObjectRef, ObjectResult, get_object_current_process},
        open_state::OpenState,
        traits::{Configuratable, MemoryMappable, Seekable, Statable},
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
const IOC_DIRBITS: u64 = 2;
const IOC_SIZEBITS: u64 = 14;
const IOC_DIRMASK: u64 = (1 << IOC_DIRBITS) - 1;
const IOC_SIZESHIFT: u64 = 16;
const IOC_DIRSHIFT: u64 = IOC_SIZESHIFT + IOC_SIZEBITS;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DrmPrimeHandleFlags: u32 {
        const CLOEXEC = 0x0008_0000;
        const RDWR = 0x0000_0002;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct DmaBufSync {
    flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct DmaBufExportSyncFile {
    flags: u32,
    fd: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct DmaBufImportSyncFile {
    flags: u32,
    fd: i32,
}

#[derive(Debug)]
pub struct DrmPrimeBufferObject {
    buffer: DumbBuffer,
    inode: u64,
    open_state: OpenState,
    position: Mutex<usize>,
}

impl DrmPrimeBufferObject {
    fn new(buffer: DumbBuffer) -> Self {
        Self {
            buffer,
            inode: NEXT_PRIME_INODE.fetch_add(1, Ordering::Relaxed),
            open_state: OpenState::default(),
            position: Mutex::new(0),
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
            ConfigurateRequest::RawIoctl { request, arg } => handle_dmabuf_ioctl(request, arg),
            other => {
                if let Some((pid, comm)) = current_debug_process() {
                    crate::s_println!(
                        "drm prime unexpected typed ioctl comm={} pid={} request={}",
                        comm,
                        pid,
                        prime_config_request_name(&other)
                    );
                }
                Err(ObjectError::InvalidRequest)
            }
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
        if let Some((pid, comm)) = current_debug_process() {
            crate::s_println!(
                "drm prime mmap comm={} pid={} offset={:#x} pages={} size={:#x} start_frame={:#x} shared_flags={:#x}",
                comm,
                pid,
                offset,
                pages,
                self.buffer.aligned_size(),
                self.buffer.start_frame_addr(),
                self.buffer.shared_flags.bits()
            );
        }
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
        *position = next.max(0) as usize;
        if let Some((pid, comm)) = current_debug_process() {
            crate::s_println!(
                "drm prime seek comm={} pid={} whence={:?} offset={} result={}",
                comm,
                pid,
                seek_type,
                offset,
                *position
            );
        }
        Ok(*position)
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
    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime_handle_to_fd comm={} pid={} handle={} flags={:#x} fd={} start_frame={:#x} map_offset={:#x}",
            comm,
            pid,
            request.handle,
            request.flags,
            request.fd,
            buffer.start_frame_addr(),
            buffer.map_offset
        );
    }
    user_safe::write(ptr, &request).map_err(|_| ObjectError::BadAddress)?;
    Ok(0)
}

fn ioc_dir(request: u64) -> u64 {
    (request >> IOC_DIRSHIFT) & IOC_DIRMASK
}

fn dma_buf_ioctl_name(request: u64) -> &'static str {
    if ioctl_type(request) != DMABUF_IOCTL_TYPE {
        return "non-dmabuf";
    }
    match ioctl_nr(request) {
        0 => "DMA_BUF_IOCTL_SYNC",
        1 => "DMA_BUF_SET_NAME",
        2 => "DMA_BUF_IOCTL_EXPORT_SYNC_FILE",
        3 => "DMA_BUF_IOCTL_IMPORT_SYNC_FILE",
        _ => "unknown-dmabuf",
    }
}

fn handle_dmabuf_ioctl(request: u64, arg: u64) -> ObjectResult<isize> {
    if ioctl_type(request) != DMABUF_IOCTL_TYPE {
        if let Some((pid, comm)) = current_debug_process() {
            crate::s_println!(
                "drm prime raw ioctl comm={} pid={} request={:#x} type={:#x} nr={:#x} dir={:#x} size={} name={} arg={:#x}",
                comm,
                pid,
                request,
                ioctl_type(request),
                ioctl_nr(request),
                ioc_dir(request),
                ioctl_size(request),
                dma_buf_ioctl_name(request),
                arg
            );
        }
        return Err(ObjectError::InvalidRequest);
    }

    match drm_prime_raw_ioctl_op(request) {
        Some(crate::object::linux_ioctl::LinuxIoctlOp::DmaBufSync) => handle_dmabuf_sync_ioctl(arg),
        Some(crate::object::linux_ioctl::LinuxIoctlOp::DmaBufExportSyncFile) => {
            handle_dmabuf_export_sync_file_ioctl(arg)
        }
        Some(crate::object::linux_ioctl::LinuxIoctlOp::DmaBufImportSyncFile) => {
            handle_dmabuf_import_sync_file_ioctl(arg)
        }
        _ => {
            if let Some((pid, comm)) = current_debug_process() {
                crate::s_println!(
                    "drm prime raw ioctl comm={} pid={} request={:#x} type={:#x} nr={:#x} dir={:#x} size={} name={} arg={:#x}",
                    comm,
                    pid,
                    request,
                    ioctl_type(request),
                    ioctl_nr(request),
                    ioc_dir(request),
                    ioctl_size(request),
                    dma_buf_ioctl_name(request),
                    arg
                );
            }
            Err(ObjectError::InvalidRequest)
        }
    }
}

fn handle_dmabuf_sync_ioctl(arg: u64) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u64 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u64 = 2;
    const DMA_BUF_SYNC_END: u64 = 1 << 2;
    const DMA_BUF_SYNC_VALID_FLAGS_MASK: u64 =
        DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE | DMA_BUF_SYNC_END;

    let sync = read_user(arg as *mut DmaBufSync)?;
    if sync.flags & !DMA_BUF_SYNC_VALID_FLAGS_MASK != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if sync.flags & (DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE) == 0 {
        return Err(ObjectError::InvalidArguments);
    }
    Ok(0)
}

fn handle_dmabuf_export_sync_file_ioctl(arg: u64) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u32 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u32 = 2;

    let mut sync_file = read_user(arg as *mut DmaBufExportSyncFile)?;
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
    user_safe::write(arg as *mut DmaBufExportSyncFile, &sync_file)
        .map_err(|_| ObjectError::BadAddress)?;

    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime export sync file comm={} pid={} flags={:#x} fd={}",
            comm,
            pid,
            sync_file.flags,
            sync_file.fd
        );
    }
    Ok(0)
}

fn handle_dmabuf_import_sync_file_ioctl(arg: u64) -> ObjectResult<isize> {
    const DMA_BUF_SYNC_READ: u32 = 1 << 0;
    const DMA_BUF_SYNC_WRITE: u32 = 2;

    let sync_file = read_user(arg as *mut DmaBufImportSyncFile)?;
    if sync_file.flags & !(DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE) != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if sync_file.flags == 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let _ =
        get_object_current_process(sync_file.fd as u64).map_err(|_| ObjectError::DoesNotExist)?;

    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime import sync file comm={} pid={} flags={:#x} fd={}",
            comm,
            pid,
            sync_file.flags,
            sync_file.fd
        );
    }
    Ok(0)
}

fn prime_config_request_name(request: &ConfigurateRequest) -> &'static str {
    match request {
        ConfigurateRequest::DrmVersion(_) => "DrmVersion",
        ConfigurateRequest::DrmGetUnique(_) => "DrmGetUnique",
        ConfigurateRequest::DrmGetMagic(_) => "DrmGetMagic",
        ConfigurateRequest::DrmGetCap(_) => "DrmGetCap",
        ConfigurateRequest::DrmWaitVblank(_) => "DrmWaitVblank",
        ConfigurateRequest::DrmSetUnique(_) => "DrmSetUnique",
        ConfigurateRequest::DrmAuthMagic(_) => "DrmAuthMagic",
        ConfigurateRequest::DrmSetClientCap(_) => "DrmSetClientCap",
        ConfigurateRequest::DrmSetMaster => "DrmSetMaster",
        ConfigurateRequest::DrmDropMaster => "DrmDropMaster",
        ConfigurateRequest::DrmModeGetResources(_) => "DrmModeGetResources",
        ConfigurateRequest::DrmModeGetCrtc(_) => "DrmModeGetCrtc",
        ConfigurateRequest::DrmModeSetCrtc(_) => "DrmModeSetCrtc",
        ConfigurateRequest::DrmModeCursor(_) => "DrmModeCursor",
        ConfigurateRequest::DrmModeCursor2(_) => "DrmModeCursor2",
        ConfigurateRequest::DrmModeGetGamma(_) => "DrmModeGetGamma",
        ConfigurateRequest::DrmModeSetGamma(_) => "DrmModeSetGamma",
        ConfigurateRequest::DrmModeGetEncoder(_) => "DrmModeGetEncoder",
        ConfigurateRequest::DrmModeGetConnector(_) => "DrmModeGetConnector",
        ConfigurateRequest::DrmModeGetProperty(_) => "DrmModeGetProperty",
        ConfigurateRequest::DrmModeObjGetProperties(_) => "DrmModeObjGetProperties",
        ConfigurateRequest::DrmModeGetPlaneResources(_) => "DrmModeGetPlaneResources",
        ConfigurateRequest::DrmModeGetPlane(_) => "DrmModeGetPlane",
        ConfigurateRequest::DrmModeListLessees(_) => "DrmModeListLessees",
        ConfigurateRequest::DrmModeAddFb(_) => "DrmModeAddFb",
        ConfigurateRequest::DrmModeAddFb2(_) => "DrmModeAddFb2",
        ConfigurateRequest::DrmModeRemoveFb(_) => "DrmModeRemoveFb",
        ConfigurateRequest::DrmModePageFlip(_) => "DrmModePageFlip",
        ConfigurateRequest::DrmModeDirtyFb(_) => "DrmModeDirtyFb",
        ConfigurateRequest::DrmModeCreateDumb(_) => "DrmModeCreateDumb",
        ConfigurateRequest::DrmModeMapDumb(_) => "DrmModeMapDumb",
        ConfigurateRequest::DrmModeDestroyDumb(_) => "DrmModeDestroyDumb",
        ConfigurateRequest::DrmGemClose(_) => "DrmGemClose",
        ConfigurateRequest::DrmPrimeHandleToFd(_) => "DrmPrimeHandleToFd",
        ConfigurateRequest::DrmPrimeFdToHandle(_) => "DrmPrimeFdToHandle",
        ConfigurateRequest::FbGetVariableScreenInfo(_) => "FbGetVariableScreenInfo",
        ConfigurateRequest::FbPutVariableScreenInfo(_) => "FbPutVariableScreenInfo",
        ConfigurateRequest::FbGetFixedScreenInfo(_) => "FbGetFixedScreenInfo",
        ConfigurateRequest::FbGetColorMap(_) => "FbGetColorMap",
        ConfigurateRequest::FbPutColorMap(_) => "FbPutColorMap",
        ConfigurateRequest::FbPanDisplay(_) => "FbPanDisplay",
        ConfigurateRequest::FbBlank(_) => "FbBlank",
        ConfigurateRequest::LinuxTcGets(_) => "LinuxTcGets",
        ConfigurateRequest::LinuxTcSets(_) => "LinuxTcSets",
        ConfigurateRequest::LinuxTcFlush(_) => "LinuxTcFlush",
        ConfigurateRequest::LinuxTcGets2(_) => "LinuxTcGets2",
        ConfigurateRequest::LinuxTcSets2(_) => "LinuxTcSets2",
        ConfigurateRequest::LinuxTiocnxcl => "LinuxTiocnxcl",
        ConfigurateRequest::LinuxTiocsctty(_) => "LinuxTiocsctty",
        ConfigurateRequest::LinuxTiocgPgrp(_) => "LinuxTiocgPgrp",
        ConfigurateRequest::LinuxTiocnotty => "LinuxTiocnotty",
        ConfigurateRequest::LinuxTiocspgrp(_) => "LinuxTiocspgrp",
        ConfigurateRequest::LinuxTiocoutq(_) => "LinuxTiocoutq",
        ConfigurateRequest::LinuxTiocgwinsz(_) => "LinuxTiocgwinsz",
        ConfigurateRequest::LinuxTiocswinsz(_) => "LinuxTiocswinsz",
        ConfigurateRequest::LinuxTiocgptn(_) => "LinuxTiocgptn",
        ConfigurateRequest::LinuxTiocsptlck(_) => "LinuxTiocsptlck",
        ConfigurateRequest::LinuxTiocgptpeer(_) => "LinuxTiocgptpeer",
        ConfigurateRequest::LinuxTiocvhangup => "LinuxTiocvhangup",
        ConfigurateRequest::LinuxKdGetKeyboardMode(_) => "LinuxKdGetKeyboardMode",
        ConfigurateRequest::LinuxKdSetKeyboardMode(_) => "LinuxKdSetKeyboardMode",
        ConfigurateRequest::LinuxKdGetKeyboardType(_) => "LinuxKdGetKeyboardType",
        ConfigurateRequest::LinuxKdGetKeyboardEntry(_) => "LinuxKdGetKeyboardEntry",
        ConfigurateRequest::LinuxKdGetDisplayMode(_) => "LinuxKdGetDisplayMode",
        ConfigurateRequest::LinuxKdSetDisplayMode(_) => "LinuxKdSetDisplayMode",
        ConfigurateRequest::LinuxKdSignalAccept(_) => "LinuxKdSignalAccept",
        ConfigurateRequest::LinuxVtOpenQuery(_) => "LinuxVtOpenQuery",
        ConfigurateRequest::LinuxVtGetMode(_) => "LinuxVtGetMode",
        ConfigurateRequest::LinuxVtGetState(_) => "LinuxVtGetState",
        ConfigurateRequest::LinuxVtSetMode(_) => "LinuxVtSetMode",
        ConfigurateRequest::LinuxVtActivate(_) => "LinuxVtActivate",
        ConfigurateRequest::LinuxVtWaitActive(_) => "LinuxVtWaitActive",
        ConfigurateRequest::LinuxVtRelDisp(_) => "LinuxVtRelDisp",
        ConfigurateRequest::RawIoctl { .. } => "RawIoctl",
    }
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
    if let Some((pid, comm)) = current_debug_process() {
        crate::s_println!(
            "drm prime_fd_to_handle comm={} pid={} fd={} handle={} start_frame={:#x} map_offset={:#x} scanout_backed={}",
            comm,
            pid,
            request.fd,
            request.handle,
            exported.start_frame_addr(),
            exported.map_offset,
            exported.scanout_backed
        );
    }
    user_safe::write(ptr, &request).map_err(|_| ObjectError::BadAddress)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{DrmPrimeHandleFlags, dma_buf_ioctl_name, ioc_dir};
    use crate::object::linux_ioctl::{ioctl_nr, ioctl_request, ioctl_size, ioctl_type};

    crate::test!(
        drm_prime_dmabuf_ioctl_decode,
        "drm prime dmabuf ioctl decode reports stable type nr dir size and names",
        drm_prime_dmabuf_ioctl_decode_reports_stable_type_nr_dir_size_and_names
    );
    crate::test!(
        drm_prime_ioctl_name_fallbacks,
        "drm prime dmabuf ioctl naming rejects non-dmabuf and unknown requests",
        drm_prime_dmabuf_ioctl_naming_rejects_non_dmabuf_and_unknown_requests
    );
    crate::test!(
        drm_prime_handle_flag_bits,
        "drm prime handle flags expose linux rdwr and cloexec bits",
        drm_prime_handle_flags_expose_linux_rdwr_and_cloexec_bits
    );

    fn drm_prime_dmabuf_ioctl_decode_reports_stable_type_nr_dir_size_and_names() {
        let request = ioctl_request(3, b'b', 2, 8);
        assert_eq!(ioctl_type(request), b'b');
        assert_eq!(ioctl_nr(request), 2);
        assert_eq!(ioctl_size(request), 8);
        assert_eq!(ioc_dir(request), 3);
        assert_eq!(
            dma_buf_ioctl_name(request),
            "DMA_BUF_IOCTL_EXPORT_SYNC_FILE"
        );
    }

    fn drm_prime_dmabuf_ioctl_naming_rejects_non_dmabuf_and_unknown_requests() {
        let non_dmabuf = ioctl_request(0, b'x', 0, 0);
        let unknown_dmabuf = ioctl_request(0, b'b', 9, 0);

        assert_eq!(dma_buf_ioctl_name(non_dmabuf), "non-dmabuf");
        assert_eq!(dma_buf_ioctl_name(unknown_dmabuf), "unknown-dmabuf");
        assert_eq!(
            dma_buf_ioctl_name(ioctl_request(1, b'b', 3, 8)),
            "DMA_BUF_IOCTL_IMPORT_SYNC_FILE"
        );
    }

    fn drm_prime_handle_flags_expose_linux_rdwr_and_cloexec_bits() {
        let flags = DrmPrimeHandleFlags::RDWR | DrmPrimeHandleFlags::CLOEXEC;
        assert!(flags.contains(DrmPrimeHandleFlags::RDWR));
        assert!(flags.contains(DrmPrimeHandleFlags::CLOEXEC));
        assert_eq!(DrmPrimeHandleFlags::RDWR.bits(), 0x0000_0002);
        assert_eq!(DrmPrimeHandleFlags::CLOEXEC.bits(), 0x0008_0000);
    }
}
