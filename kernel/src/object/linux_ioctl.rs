#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinuxIoctlTarget {
    Framebuffer,
    Terminal,
    TtyDevice,
    PtyMaster,
    PtySlave,
    DrmCard,
    DrmPrime,
    EvdevClient,
    UnixSocket,
    InetSocket,
    NetlinkSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinuxIoctlOp {
    FbGetVariableScreenInfo,
    FbPutVariableScreenInfo,
    FbGetFixedScreenInfo,
    FbGetColorMap,
    FbPutColorMap,
    FbPanDisplay,
    FbBlank,
    LinuxTcGets,
    LinuxTcSets,
    LinuxTcFlush,
    LinuxTcGets2,
    LinuxTcSets2,
    LinuxTiocnxcl,
    LinuxTiocsctty,
    LinuxTiocgPgrp,
    LinuxTiocnotty,
    LinuxTiocspgrp,
    LinuxTiocoutq,
    LinuxTiocgwinsz,
    LinuxTiocswinsz,
    LinuxTiocgptn,
    LinuxTiocsptlck,
    LinuxTiocgptpeer,
    LinuxTiocvhangup,
    LinuxKdGetKeyboardMode,
    LinuxKdSetKeyboardMode,
    LinuxKdGetKeyboardType,
    LinuxKdGetKeyboardEntry,
    LinuxKdGetDisplayMode,
    LinuxKdSetDisplayMode,
    LinuxKdSignalAccept,
    LinuxVtOpenQuery,
    LinuxVtGetMode,
    LinuxVtGetState,
    LinuxVtSetMode,
    LinuxVtActivate,
    LinuxVtWaitActive,
    LinuxVtRelDisp,
    DrmVersion,
    DrmGetUnique,
    DrmGetMagic,
    DrmGetCap,
    DrmWaitVblank,
    DrmSetUnique,
    DrmAuthMagic,
    DrmSetClientCap,
    DrmSetMaster,
    DrmDropMaster,
    DrmModeGetResources,
    DrmModeGetCrtc,
    DrmModeSetCrtc,
    DrmModeCursor,
    DrmModeCursor2,
    DrmModeGetGamma,
    DrmModeSetGamma,
    DrmModeGetEncoder,
    DrmModeGetConnector,
    DrmModeGetProperty,
    DrmModeObjGetProperties,
    DrmModeGetPlaneResources,
    DrmModeGetPlane,
    DrmModeListLessees,
    DrmModeAddFb,
    DrmModeAddFb2,
    DrmModeRemoveFb,
    DrmModePageFlip,
    DrmModeDirtyFb,
    DrmModeCreateDumb,
    DrmModeMapDumb,
    DrmModeDestroyDumb,
    DrmGemClose,
    DrmPrimeHandleToFd,
    DrmPrimeFdToHandle,
    RawFionbio,
    RawFioclex,
    DmaBufSync,
    DmaBufExportSyncFile,
    DmaBufImportSyncFile,
    EvdevGetVersion,
    EvdevGetId,
    EvdevGetRepeat,
    EvdevGetName,
    EvdevGetPhys,
    EvdevGetUniq,
    EvdevGetProp,
    EvdevGetKey,
    EvdevGetLed,
    EvdevGetSnd,
    EvdevGetSw,
    EvdevGetBit,
    EvdevGrab,
    EvdevRevoke,
    EvdevSetClockId,
}

pub const RAW_IOCTL_FIONBIO: u64 = 0x5421;
pub const RAW_IOCTL_FIOCLEX: u64 = 0x5451;

const IOC_NRBITS: u64 = 8;
const IOC_TYPEBITS: u64 = 8;
const IOC_SIZEBITS: u64 = 14;
const IOC_NRSHIFT: u64 = 0;
const IOC_TYPESHIFT: u64 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u64 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u64 = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_NRMASK: u64 = (1 << IOC_NRBITS) - 1;
const IOC_TYPEMASK: u64 = (1 << IOC_TYPEBITS) - 1;
const IOC_SIZEMASK: u64 = (1 << IOC_SIZEBITS) - 1;

pub const EVDEV_IOCTL_TYPE: u8 = b'E';
pub const DMABUF_IOCTL_TYPE: u8 = b'b';

pub const fn ioctl_request(dir: u64, ty: u8, nr: u8, size: usize) -> u64 {
    (nr as u64)
        | ((ty as u64) << IOC_TYPESHIFT)
        | ((size as u64) << IOC_SIZESHIFT)
        | (dir << IOC_DIRSHIFT)
}

pub const fn ioctl_nr(request: u64) -> u64 {
    (request >> IOC_NRSHIFT) & IOC_NRMASK
}

pub const fn ioctl_type(request: u64) -> u8 {
    ((request >> IOC_TYPESHIFT) & IOC_TYPEMASK) as u8
}

pub const fn ioctl_size(request: u64) -> usize {
    ((request >> IOC_SIZESHIFT) & IOC_SIZEMASK) as usize
}

pub fn socket_raw_ioctl_op(request: u64) -> Option<LinuxIoctlOp> {
    match request {
        RAW_IOCTL_FIONBIO => Some(LinuxIoctlOp::RawFionbio),
        RAW_IOCTL_FIOCLEX => Some(LinuxIoctlOp::RawFioclex),
        _ => None,
    }
}

pub fn drm_prime_raw_ioctl_op(request: u64) -> Option<LinuxIoctlOp> {
    if ioctl_type(request) != DMABUF_IOCTL_TYPE {
        return None;
    }

    match ioctl_nr(request) {
        0 => Some(LinuxIoctlOp::DmaBufSync),
        2 => Some(LinuxIoctlOp::DmaBufExportSyncFile),
        3 => Some(LinuxIoctlOp::DmaBufImportSyncFile),
        _ => None,
    }
}

pub fn evdev_raw_ioctl_op(request: u64) -> Option<LinuxIoctlOp> {
    if ioctl_type(request) != EVDEV_IOCTL_TYPE {
        return None;
    }

    match ioctl_nr(request) {
        0x01 => Some(LinuxIoctlOp::EvdevGetVersion),
        0x02 => Some(LinuxIoctlOp::EvdevGetId),
        0x03 => Some(LinuxIoctlOp::EvdevGetRepeat),
        0x06 => Some(LinuxIoctlOp::EvdevGetName),
        0x07 => Some(LinuxIoctlOp::EvdevGetPhys),
        0x08 => Some(LinuxIoctlOp::EvdevGetUniq),
        0x09 => Some(LinuxIoctlOp::EvdevGetProp),
        0x18 => Some(LinuxIoctlOp::EvdevGetKey),
        0x19 => Some(LinuxIoctlOp::EvdevGetLed),
        0x1a => Some(LinuxIoctlOp::EvdevGetSnd),
        0x1b => Some(LinuxIoctlOp::EvdevGetSw),
        0x20..=0x3f => Some(LinuxIoctlOp::EvdevGetBit),
        0x90 => Some(LinuxIoctlOp::EvdevGrab),
        0x91 => Some(LinuxIoctlOp::EvdevRevoke),
        0xa0 => Some(LinuxIoctlOp::EvdevSetClockId),
        _ => None,
    }
}
