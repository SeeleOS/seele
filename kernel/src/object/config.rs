use bitflags::bitflags;

use crate::{
    drm::prime::{DmaBufExportSyncFile, DmaBufImportSyncFile, DmaBufSync},
    object::linux_ioctl::LinuxIoctlOp,
    object::{
        FileFlags, ObjectResult,
        error::ObjectError,
        linux_ioctl::{drm_prime_raw_ioctl_op, evdev_raw_ioctl_op, ioctl_nr, ioctl_size},
    },
    process::FdFlags,
};

use crate::drm::{
    client::{
        DRM_IOCTL_AUTH_MAGIC, DRM_IOCTL_DROP_MASTER, DRM_IOCTL_GEM_CLOSE, DRM_IOCTL_GET_CAP,
        DRM_IOCTL_GET_MAGIC, DRM_IOCTL_GET_UNIQUE, DRM_IOCTL_PRIME_FD_TO_HANDLE,
        DRM_IOCTL_PRIME_HANDLE_TO_FD, DRM_IOCTL_SET_CLIENT_CAP, DRM_IOCTL_SET_MASTER,
        DRM_IOCTL_SET_UNIQUE, DRM_IOCTL_VERSION, DRM_IOCTL_WAIT_VBLANK, DrmAuth, DrmGemClose,
        DrmGetCap, DrmPrimeHandle, DrmSetClientCap, DrmUnique, DrmVersion, DrmWaitVblank,
    },
    mode::{
        DRM_IOCTL_MODE_ADDFB, DRM_IOCTL_MODE_ADDFB2, DRM_IOCTL_MODE_CREATE_DUMB,
        DRM_IOCTL_MODE_CURSOR, DRM_IOCTL_MODE_CURSOR2, DRM_IOCTL_MODE_DESTROY_DUMB,
        DRM_IOCTL_MODE_DIRTYFB, DRM_IOCTL_MODE_GETCONNECTOR, DRM_IOCTL_MODE_GETCRTC,
        DRM_IOCTL_MODE_GETENCODER, DRM_IOCTL_MODE_GETGAMMA, DRM_IOCTL_MODE_GETPLANE,
        DRM_IOCTL_MODE_GETPLANERESOURCES, DRM_IOCTL_MODE_GETPROPERTY, DRM_IOCTL_MODE_GETRESOURCES,
        DRM_IOCTL_MODE_LIST_LESSEES, DRM_IOCTL_MODE_MAP_DUMB, DRM_IOCTL_MODE_OBJ_GETPROPERTIES,
        DRM_IOCTL_MODE_PAGE_FLIP, DRM_IOCTL_MODE_RMFB, DRM_IOCTL_MODE_SETCRTC,
        DRM_IOCTL_MODE_SETGAMMA,
    },
    mode_types::{
        DrmModeCardRes, DrmModeCreateDumb, DrmModeCrtc, DrmModeCrtcLut, DrmModeCrtcPageFlip,
        DrmModeCursor, DrmModeCursor2, DrmModeDestroyDumb, DrmModeFbCmd, DrmModeFbCmd2,
        DrmModeFbDirtyCmd, DrmModeGetConnector, DrmModeGetEncoder, DrmModeGetPlane,
        DrmModeGetPlaneRes, DrmModeGetProperty, DrmModeListLessees, DrmModeMapDumb,
        DrmModeObjGetProperties,
    },
};
use crate::misc::framebuffer_ioctl::{FbCmap, FbFixScreeninfo, FbVarScreeninfo};
use crate::terminal::linux_kd::{LinuxKbEntry, LinuxVtMode, LinuxVtStat};

pub enum ConfigurateRequest {
    FbGetVariableScreenInfo(*mut FbVarScreeninfo),
    FbPutVariableScreenInfo(*mut FbVarScreeninfo),
    FbGetFixedScreenInfo(*mut FbFixScreeninfo),
    FbGetColorMap(*mut FbCmap),
    FbPutColorMap(*mut FbCmap),
    FbPanDisplay(*mut FbVarScreeninfo),
    FbBlank(u32),
    LinuxTcGets(*mut LinuxTermios),
    LinuxTcSets(*const LinuxTermios),
    LinuxTcFlush(u32),
    LinuxTcGets2(*mut LinuxTermios2),
    LinuxTcSets2(*const LinuxTermios2),
    LinuxTiocnxcl,
    LinuxTiocsctty(u32),
    LinuxTiocgPgrp(*mut i32),
    LinuxTiocnotty,
    LinuxTiocspgrp(*const i32),
    LinuxTiocoutq(*mut i32),
    LinuxTiocgwinsz(*mut LinuxWinsize),
    LinuxTiocswinsz(*const LinuxWinsize),
    LinuxTiocgptn(*mut i32),
    LinuxTiocsptlck(*const i32),
    LinuxTiocgptpeer(PtyPeerOpenRequest),
    LinuxTiocvhangup,
    LinuxKdGetKeyboardMode(*mut u32),
    LinuxKdSetKeyboardMode(u32),
    LinuxKdGetKeyboardType(*mut u8),
    LinuxKdGetKeyboardEntry(*mut LinuxKbEntry),
    LinuxKdGetDisplayMode(*mut u32),
    LinuxKdSetDisplayMode(u32),
    LinuxKdSignalAccept(u32),
    LinuxVtOpenQuery(*mut u32),
    LinuxVtGetMode(*mut LinuxVtMode),
    LinuxVtGetState(*mut LinuxVtStat),
    LinuxVtSetMode(*const LinuxVtMode),
    LinuxVtActivate(u32),
    LinuxVtWaitActive(u32),
    LinuxVtRelDisp(u32),
    DrmVersion(*mut DrmVersion),
    DrmGetUnique(*mut DrmUnique),
    DrmGetMagic(*mut DrmAuth),
    DrmGetCap(*mut DrmGetCap),
    DrmWaitVblank(*mut DrmWaitVblank),
    DrmSetUnique(*mut DrmUnique),
    DrmAuthMagic(*mut DrmAuth),
    DrmSetClientCap(*mut DrmSetClientCap),
    DrmSetMaster,
    DrmDropMaster,
    DrmModeGetResources(*mut DrmModeCardRes),
    DrmModeGetCrtc(*mut DrmModeCrtc),
    DrmModeSetCrtc(*mut DrmModeCrtc),
    DrmModeCursor(*mut DrmModeCursor),
    DrmModeCursor2(*mut DrmModeCursor2),
    DrmModeGetGamma(*mut DrmModeCrtcLut),
    DrmModeSetGamma(*mut DrmModeCrtcLut),
    DrmModeGetEncoder(*mut DrmModeGetEncoder),
    DrmModeGetConnector(*mut DrmModeGetConnector),
    DrmModeGetProperty(*mut DrmModeGetProperty),
    DrmModeObjGetProperties(*mut DrmModeObjGetProperties),
    DrmModeGetPlaneResources(*mut DrmModeGetPlaneRes),
    DrmModeGetPlane(*mut DrmModeGetPlane),
    DrmModeListLessees(*mut DrmModeListLessees),
    DrmModeAddFb(*mut DrmModeFbCmd),
    DrmModeAddFb2(*mut DrmModeFbCmd2),
    DrmModeRemoveFb(*mut u32),
    DrmModePageFlip(*mut DrmModeCrtcPageFlip),
    DrmModeDirtyFb(*mut DrmModeFbDirtyCmd),
    DrmModeCreateDumb(*mut DrmModeCreateDumb),
    DrmModeMapDumb(*mut DrmModeMapDumb),
    DrmModeDestroyDumb(*mut DrmModeDestroyDumb),
    DrmGemClose(*mut DrmGemClose),
    DrmPrimeHandleToFd(*mut DrmPrimeHandle),
    DrmPrimeFdToHandle(*mut DrmPrimeHandle),
    DmaBufSync(*mut DmaBufSync),
    DmaBufExportSyncFile(*mut DmaBufExportSyncFile),
    DmaBufImportSyncFile(*mut DmaBufImportSyncFile),
    EvdevGetVersion(*mut i32),
    EvdevGetId(*mut u8),
    EvdevGetRepeat(*mut [u32; 2]),
    EvdevGetName {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetPhys {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetUniq {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetProp {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetKey {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetLed {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetSnd {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetSw {
        ptr: *mut u8,
        len: usize,
    },
    EvdevGetBit {
        event_type: u8,
        ptr: *mut u8,
        len: usize,
    },
    EvdevGrab(u64),
    EvdevRevoke(u64),
    EvdevSetClockId(*const i32),
    RawIoctl {
        request: u64,
        arg: u64,
    },
}

impl ConfigurateRequest {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FbGetVariableScreenInfo(_) => "FbGetVariableScreenInfo",
            Self::FbPutVariableScreenInfo(_) => "FbPutVariableScreenInfo",
            Self::FbGetFixedScreenInfo(_) => "FbGetFixedScreenInfo",
            Self::FbGetColorMap(_) => "FbGetColorMap",
            Self::FbPutColorMap(_) => "FbPutColorMap",
            Self::FbPanDisplay(_) => "FbPanDisplay",
            Self::FbBlank(_) => "FbBlank",
            Self::LinuxTcGets(_) => "LinuxTcGets",
            Self::LinuxTcSets(_) => "LinuxTcSets",
            Self::LinuxTcFlush(_) => "LinuxTcFlush",
            Self::LinuxTcGets2(_) => "LinuxTcGets2",
            Self::LinuxTcSets2(_) => "LinuxTcSets2",
            Self::LinuxTiocnxcl => "LinuxTiocnxcl",
            Self::LinuxTiocsctty(_) => "LinuxTiocsctty",
            Self::LinuxTiocgPgrp(_) => "LinuxTiocgPgrp",
            Self::LinuxTiocnotty => "LinuxTiocnotty",
            Self::LinuxTiocspgrp(_) => "LinuxTiocspgrp",
            Self::LinuxTiocoutq(_) => "LinuxTiocoutq",
            Self::LinuxTiocgwinsz(_) => "LinuxTiocgwinsz",
            Self::LinuxTiocswinsz(_) => "LinuxTiocswinsz",
            Self::LinuxTiocgptn(_) => "LinuxTiocgptn",
            Self::LinuxTiocsptlck(_) => "LinuxTiocsptlck",
            Self::LinuxTiocgptpeer(_) => "LinuxTiocgptpeer",
            Self::LinuxTiocvhangup => "LinuxTiocvhangup",
            Self::LinuxKdGetKeyboardMode(_) => "LinuxKdGetKeyboardMode",
            Self::LinuxKdSetKeyboardMode(_) => "LinuxKdSetKeyboardMode",
            Self::LinuxKdGetKeyboardType(_) => "LinuxKdGetKeyboardType",
            Self::LinuxKdGetKeyboardEntry(_) => "LinuxKdGetKeyboardEntry",
            Self::LinuxKdGetDisplayMode(_) => "LinuxKdGetDisplayMode",
            Self::LinuxKdSetDisplayMode(_) => "LinuxKdSetDisplayMode",
            Self::LinuxKdSignalAccept(_) => "LinuxKdSignalAccept",
            Self::LinuxVtOpenQuery(_) => "LinuxVtOpenQuery",
            Self::LinuxVtGetMode(_) => "LinuxVtGetMode",
            Self::LinuxVtGetState(_) => "LinuxVtGetState",
            Self::LinuxVtSetMode(_) => "LinuxVtSetMode",
            Self::LinuxVtActivate(_) => "LinuxVtActivate",
            Self::LinuxVtWaitActive(_) => "LinuxVtWaitActive",
            Self::LinuxVtRelDisp(_) => "LinuxVtRelDisp",
            Self::DrmVersion(_) => "DrmVersion",
            Self::DrmGetUnique(_) => "DrmGetUnique",
            Self::DrmGetMagic(_) => "DrmGetMagic",
            Self::DrmGetCap(_) => "DrmGetCap",
            Self::DrmWaitVblank(_) => "DrmWaitVblank",
            Self::DrmSetUnique(_) => "DrmSetUnique",
            Self::DrmAuthMagic(_) => "DrmAuthMagic",
            Self::DrmSetClientCap(_) => "DrmSetClientCap",
            Self::DrmSetMaster => "DrmSetMaster",
            Self::DrmDropMaster => "DrmDropMaster",
            Self::DrmModeGetResources(_) => "DrmModeGetResources",
            Self::DrmModeGetCrtc(_) => "DrmModeGetCrtc",
            Self::DrmModeSetCrtc(_) => "DrmModeSetCrtc",
            Self::DrmModeCursor(_) => "DrmModeCursor",
            Self::DrmModeCursor2(_) => "DrmModeCursor2",
            Self::DrmModeGetGamma(_) => "DrmModeGetGamma",
            Self::DrmModeSetGamma(_) => "DrmModeSetGamma",
            Self::DrmModeGetEncoder(_) => "DrmModeGetEncoder",
            Self::DrmModeGetConnector(_) => "DrmModeGetConnector",
            Self::DrmModeGetProperty(_) => "DrmModeGetProperty",
            Self::DrmModeObjGetProperties(_) => "DrmModeObjGetProperties",
            Self::DrmModeGetPlaneResources(_) => "DrmModeGetPlaneResources",
            Self::DrmModeGetPlane(_) => "DrmModeGetPlane",
            Self::DrmModeListLessees(_) => "DrmModeListLessees",
            Self::DrmModeAddFb(_) => "DrmModeAddFb",
            Self::DrmModeAddFb2(_) => "DrmModeAddFb2",
            Self::DrmModeRemoveFb(_) => "DrmModeRemoveFb",
            Self::DrmModePageFlip(_) => "DrmModePageFlip",
            Self::DrmModeDirtyFb(_) => "DrmModeDirtyFb",
            Self::DrmModeCreateDumb(_) => "DrmModeCreateDumb",
            Self::DrmModeMapDumb(_) => "DrmModeMapDumb",
            Self::DrmModeDestroyDumb(_) => "DrmModeDestroyDumb",
            Self::DrmGemClose(_) => "DrmGemClose",
            Self::DrmPrimeHandleToFd(_) => "DrmPrimeHandleToFd",
            Self::DrmPrimeFdToHandle(_) => "DrmPrimeFdToHandle",
            Self::DmaBufSync(_) => "DmaBufSync",
            Self::DmaBufExportSyncFile(_) => "DmaBufExportSyncFile",
            Self::DmaBufImportSyncFile(_) => "DmaBufImportSyncFile",
            Self::EvdevGetVersion(_) => "EvdevGetVersion",
            Self::EvdevGetId(_) => "EvdevGetId",
            Self::EvdevGetRepeat(_) => "EvdevGetRepeat",
            Self::EvdevGetName { .. } => "EvdevGetName",
            Self::EvdevGetPhys { .. } => "EvdevGetPhys",
            Self::EvdevGetUniq { .. } => "EvdevGetUniq",
            Self::EvdevGetProp { .. } => "EvdevGetProp",
            Self::EvdevGetKey { .. } => "EvdevGetKey",
            Self::EvdevGetLed { .. } => "EvdevGetLed",
            Self::EvdevGetSnd { .. } => "EvdevGetSnd",
            Self::EvdevGetSw { .. } => "EvdevGetSw",
            Self::EvdevGetBit { .. } => "EvdevGetBit",
            Self::EvdevGrab(_) => "EvdevGrab",
            Self::EvdevRevoke(_) => "EvdevRevoke",
            Self::EvdevSetClockId(_) => "EvdevSetClockId",
            Self::RawIoctl { .. } => "RawIoctl",
        }
    }

    pub fn kind(&self) -> Option<LinuxIoctlOp> {
        Some(match self {
            Self::FbGetVariableScreenInfo(_) => LinuxIoctlOp::FbGetVariableScreenInfo,
            Self::FbPutVariableScreenInfo(_) => LinuxIoctlOp::FbPutVariableScreenInfo,
            Self::FbGetFixedScreenInfo(_) => LinuxIoctlOp::FbGetFixedScreenInfo,
            Self::FbGetColorMap(_) => LinuxIoctlOp::FbGetColorMap,
            Self::FbPutColorMap(_) => LinuxIoctlOp::FbPutColorMap,
            Self::FbPanDisplay(_) => LinuxIoctlOp::FbPanDisplay,
            Self::FbBlank(_) => LinuxIoctlOp::FbBlank,
            Self::LinuxTcGets(_) => LinuxIoctlOp::LinuxTcGets,
            Self::LinuxTcSets(_) => LinuxIoctlOp::LinuxTcSets,
            Self::LinuxTcFlush(_) => LinuxIoctlOp::LinuxTcFlush,
            Self::LinuxTcGets2(_) => LinuxIoctlOp::LinuxTcGets2,
            Self::LinuxTcSets2(_) => LinuxIoctlOp::LinuxTcSets2,
            Self::LinuxTiocnxcl => LinuxIoctlOp::LinuxTiocnxcl,
            Self::LinuxTiocsctty(_) => LinuxIoctlOp::LinuxTiocsctty,
            Self::LinuxTiocgPgrp(_) => LinuxIoctlOp::LinuxTiocgPgrp,
            Self::LinuxTiocnotty => LinuxIoctlOp::LinuxTiocnotty,
            Self::LinuxTiocspgrp(_) => LinuxIoctlOp::LinuxTiocspgrp,
            Self::LinuxTiocoutq(_) => LinuxIoctlOp::LinuxTiocoutq,
            Self::LinuxTiocgwinsz(_) => LinuxIoctlOp::LinuxTiocgwinsz,
            Self::LinuxTiocswinsz(_) => LinuxIoctlOp::LinuxTiocswinsz,
            Self::LinuxTiocgptn(_) => LinuxIoctlOp::LinuxTiocgptn,
            Self::LinuxTiocsptlck(_) => LinuxIoctlOp::LinuxTiocsptlck,
            Self::LinuxTiocgptpeer(_) => LinuxIoctlOp::LinuxTiocgptpeer,
            Self::LinuxTiocvhangup => LinuxIoctlOp::LinuxTiocvhangup,
            Self::LinuxKdGetKeyboardMode(_) => LinuxIoctlOp::LinuxKdGetKeyboardMode,
            Self::LinuxKdSetKeyboardMode(_) => LinuxIoctlOp::LinuxKdSetKeyboardMode,
            Self::LinuxKdGetKeyboardType(_) => LinuxIoctlOp::LinuxKdGetKeyboardType,
            Self::LinuxKdGetKeyboardEntry(_) => LinuxIoctlOp::LinuxKdGetKeyboardEntry,
            Self::LinuxKdGetDisplayMode(_) => LinuxIoctlOp::LinuxKdGetDisplayMode,
            Self::LinuxKdSetDisplayMode(_) => LinuxIoctlOp::LinuxKdSetDisplayMode,
            Self::LinuxKdSignalAccept(_) => LinuxIoctlOp::LinuxKdSignalAccept,
            Self::LinuxVtOpenQuery(_) => LinuxIoctlOp::LinuxVtOpenQuery,
            Self::LinuxVtGetMode(_) => LinuxIoctlOp::LinuxVtGetMode,
            Self::LinuxVtGetState(_) => LinuxIoctlOp::LinuxVtGetState,
            Self::LinuxVtSetMode(_) => LinuxIoctlOp::LinuxVtSetMode,
            Self::LinuxVtActivate(_) => LinuxIoctlOp::LinuxVtActivate,
            Self::LinuxVtWaitActive(_) => LinuxIoctlOp::LinuxVtWaitActive,
            Self::LinuxVtRelDisp(_) => LinuxIoctlOp::LinuxVtRelDisp,
            Self::DrmVersion(_) => LinuxIoctlOp::DrmVersion,
            Self::DrmGetUnique(_) => LinuxIoctlOp::DrmGetUnique,
            Self::DrmGetMagic(_) => LinuxIoctlOp::DrmGetMagic,
            Self::DrmGetCap(_) => LinuxIoctlOp::DrmGetCap,
            Self::DrmWaitVblank(_) => LinuxIoctlOp::DrmWaitVblank,
            Self::DrmSetUnique(_) => LinuxIoctlOp::DrmSetUnique,
            Self::DrmAuthMagic(_) => LinuxIoctlOp::DrmAuthMagic,
            Self::DrmSetClientCap(_) => LinuxIoctlOp::DrmSetClientCap,
            Self::DrmSetMaster => LinuxIoctlOp::DrmSetMaster,
            Self::DrmDropMaster => LinuxIoctlOp::DrmDropMaster,
            Self::DrmModeGetResources(_) => LinuxIoctlOp::DrmModeGetResources,
            Self::DrmModeGetCrtc(_) => LinuxIoctlOp::DrmModeGetCrtc,
            Self::DrmModeSetCrtc(_) => LinuxIoctlOp::DrmModeSetCrtc,
            Self::DrmModeCursor(_) => LinuxIoctlOp::DrmModeCursor,
            Self::DrmModeCursor2(_) => LinuxIoctlOp::DrmModeCursor2,
            Self::DrmModeGetGamma(_) => LinuxIoctlOp::DrmModeGetGamma,
            Self::DrmModeSetGamma(_) => LinuxIoctlOp::DrmModeSetGamma,
            Self::DrmModeGetEncoder(_) => LinuxIoctlOp::DrmModeGetEncoder,
            Self::DrmModeGetConnector(_) => LinuxIoctlOp::DrmModeGetConnector,
            Self::DrmModeGetProperty(_) => LinuxIoctlOp::DrmModeGetProperty,
            Self::DrmModeObjGetProperties(_) => LinuxIoctlOp::DrmModeObjGetProperties,
            Self::DrmModeGetPlaneResources(_) => LinuxIoctlOp::DrmModeGetPlaneResources,
            Self::DrmModeGetPlane(_) => LinuxIoctlOp::DrmModeGetPlane,
            Self::DrmModeListLessees(_) => LinuxIoctlOp::DrmModeListLessees,
            Self::DrmModeAddFb(_) => LinuxIoctlOp::DrmModeAddFb,
            Self::DrmModeAddFb2(_) => LinuxIoctlOp::DrmModeAddFb2,
            Self::DrmModeRemoveFb(_) => LinuxIoctlOp::DrmModeRemoveFb,
            Self::DrmModePageFlip(_) => LinuxIoctlOp::DrmModePageFlip,
            Self::DrmModeDirtyFb(_) => LinuxIoctlOp::DrmModeDirtyFb,
            Self::DrmModeCreateDumb(_) => LinuxIoctlOp::DrmModeCreateDumb,
            Self::DrmModeMapDumb(_) => LinuxIoctlOp::DrmModeMapDumb,
            Self::DrmModeDestroyDumb(_) => LinuxIoctlOp::DrmModeDestroyDumb,
            Self::DrmGemClose(_) => LinuxIoctlOp::DrmGemClose,
            Self::DrmPrimeHandleToFd(_) => LinuxIoctlOp::DrmPrimeHandleToFd,
            Self::DrmPrimeFdToHandle(_) => LinuxIoctlOp::DrmPrimeFdToHandle,
            Self::DmaBufSync(_) => LinuxIoctlOp::DmaBufSync,
            Self::DmaBufExportSyncFile(_) => LinuxIoctlOp::DmaBufExportSyncFile,
            Self::DmaBufImportSyncFile(_) => LinuxIoctlOp::DmaBufImportSyncFile,
            Self::EvdevGetVersion(_) => LinuxIoctlOp::EvdevGetVersion,
            Self::EvdevGetId(_) => LinuxIoctlOp::EvdevGetId,
            Self::EvdevGetRepeat(_) => LinuxIoctlOp::EvdevGetRepeat,
            Self::EvdevGetName { .. } => LinuxIoctlOp::EvdevGetName,
            Self::EvdevGetPhys { .. } => LinuxIoctlOp::EvdevGetPhys,
            Self::EvdevGetUniq { .. } => LinuxIoctlOp::EvdevGetUniq,
            Self::EvdevGetProp { .. } => LinuxIoctlOp::EvdevGetProp,
            Self::EvdevGetKey { .. } => LinuxIoctlOp::EvdevGetKey,
            Self::EvdevGetLed { .. } => LinuxIoctlOp::EvdevGetLed,
            Self::EvdevGetSnd { .. } => LinuxIoctlOp::EvdevGetSnd,
            Self::EvdevGetSw { .. } => LinuxIoctlOp::EvdevGetSw,
            Self::EvdevGetBit { .. } => LinuxIoctlOp::EvdevGetBit,
            Self::EvdevGrab(_) => LinuxIoctlOp::EvdevGrab,
            Self::EvdevRevoke(_) => LinuxIoctlOp::EvdevRevoke,
            Self::EvdevSetClockId(_) => LinuxIoctlOp::EvdevSetClockId,
            Self::RawIoctl { .. } => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigurateRequest, PtyPeerAccessMode, PtyPeerOpenFlags, PtyPeerOpenRequest};
    use crate::object::{FileFlags, error::ObjectError};
    use crate::process::FdFlags;

    crate::test!(
        pty_peer_open_request_parse,
        "pty peer open request parses access mode and translates cloexec nonblock flags",
        pty_peer_open_request_parses_access_mode_and_translates_cloexec_nonblock_flags
    );
    crate::test!(
        pty_peer_open_request_rejects_invalid_bits,
        "pty peer open request rejects unsupported access modes and flag bits",
        pty_peer_open_request_rejects_unsupported_access_modes_and_flag_bits
    );
    crate::test!(
        configurate_request_routes_linux_terminal_and_drm_ioctl_numbers,
        "configurate request routes known linux terminal and drm ioctl numbers before raw fallback",
        configurate_request_routes_known_linux_terminal_and_drm_ioctl_numbers_before_raw_fallback
    );

    fn pty_peer_open_request_parses_access_mode_and_translates_cloexec_nonblock_flags() {
        let request = PtyPeerOpenRequest::new(
            2 | PtyPeerOpenFlags::O_NONBLOCK.bits() as u64
                | PtyPeerOpenFlags::O_CLOEXEC.bits() as u64,
        )
        .unwrap();

        assert!(matches!(request.access_mode, PtyPeerAccessMode::ReadWrite));
        assert_eq!(request.fd_flags(), FdFlags::CLOEXEC);
        assert_eq!(request.file_flags(), FileFlags::NONBLOCK);
    }

    fn pty_peer_open_request_rejects_unsupported_access_modes_and_flag_bits() {
        assert!(matches!(
            PtyPeerOpenRequest::new(3),
            Err(ObjectError::InvalidArguments)
        ));
        assert!(matches!(
            PtyPeerOpenRequest::new(2 | (1u64 << 63)),
            Err(ObjectError::InvalidArguments)
        ));
    }

    fn configurate_request_routes_known_linux_terminal_and_drm_ioctl_numbers_before_raw_fallback() {
        assert!(matches!(
            ConfigurateRequest::new(0x5413, 0x1234).unwrap(),
            ConfigurateRequest::LinuxTiocgwinsz(ptr) if ptr as usize == 0x1234
        ));
        assert!(matches!(
            ConfigurateRequest::new(0x5441, 2).unwrap(),
            ConfigurateRequest::LinuxTiocgptpeer(_)
        ));
        assert!(matches!(
            ConfigurateRequest::new(0x5607, 4).unwrap(),
            ConfigurateRequest::LinuxVtWaitActive(4)
        ));
        assert!(matches!(
            ConfigurateRequest::new(crate::drm::client::DRM_IOCTL_GET_CAP, 0xfeed).unwrap(),
            ConfigurateRequest::DrmGetCap(ptr) if ptr as usize == 0xfeed
        ));
        assert!(matches!(
            ConfigurateRequest::new(0xdead_beef, 0xbeef).unwrap(),
            ConfigurateRequest::RawIoctl {
                request: 0xdead_beef,
                arg: 0xbeef
            }
        ));
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct PtyPeerOpenFlags: u32 {
        const O_NOCTTY = 0x100;
        const O_NONBLOCK = 0o4_000;
        const O_CLOEXEC = 0o2_000_000;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PtyPeerAccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug)]
pub struct PtyPeerOpenRequest {
    pub access_mode: PtyPeerAccessMode,
    pub flags: PtyPeerOpenFlags,
}

impl PtyPeerOpenRequest {
    const ACCESS_MODE_MASK: u64 = 0o3;

    fn new(raw: u64) -> ObjectResult<Self> {
        let access_mode = match raw & Self::ACCESS_MODE_MASK {
            0 => PtyPeerAccessMode::ReadOnly,
            1 => PtyPeerAccessMode::WriteOnly,
            2 => PtyPeerAccessMode::ReadWrite,
            _ => return Err(ObjectError::InvalidArguments),
        };

        let flag_bits = u32::try_from(raw & !Self::ACCESS_MODE_MASK)
            .map_err(|_| ObjectError::InvalidArguments)?;
        let flags = PtyPeerOpenFlags::from_bits(flag_bits).ok_or_else(|| {
            crate::s_println!("unsupported pty peer open flags raw={:#x}", flag_bits);
            ObjectError::InvalidArguments
        })?;

        Ok(Self { access_mode, flags })
    }

    pub fn fd_flags(self) -> FdFlags {
        if self.flags.contains(PtyPeerOpenFlags::O_CLOEXEC) {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        }
    }

    pub fn file_flags(self) -> FileFlags {
        let mut file_flags = FileFlags::empty();
        if self.flags.contains(PtyPeerOpenFlags::O_NONBLOCK) {
            file_flags.insert(FileFlags::NONBLOCK);
        }
        file_flags
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTermios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxTermios2 {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxWinsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        Ok(match request {
            DRM_IOCTL_VERSION => Self::DrmVersion(ptr as *mut DrmVersion),
            DRM_IOCTL_GET_UNIQUE => Self::DrmGetUnique(ptr as *mut DrmUnique),
            DRM_IOCTL_GET_MAGIC => Self::DrmGetMagic(ptr as *mut DrmAuth),
            DRM_IOCTL_GET_CAP => Self::DrmGetCap(ptr as *mut DrmGetCap),
            DRM_IOCTL_WAIT_VBLANK => Self::DrmWaitVblank(ptr as *mut DrmWaitVblank),
            DRM_IOCTL_SET_UNIQUE => Self::DrmSetUnique(ptr as *mut DrmUnique),
            DRM_IOCTL_AUTH_MAGIC => Self::DrmAuthMagic(ptr as *mut DrmAuth),
            DRM_IOCTL_SET_CLIENT_CAP => Self::DrmSetClientCap(ptr as *mut DrmSetClientCap),
            DRM_IOCTL_SET_MASTER => Self::DrmSetMaster,
            DRM_IOCTL_DROP_MASTER => Self::DrmDropMaster,
            DRM_IOCTL_MODE_GETRESOURCES => Self::DrmModeGetResources(ptr as *mut DrmModeCardRes),
            DRM_IOCTL_MODE_GETCRTC => Self::DrmModeGetCrtc(ptr as *mut DrmModeCrtc),
            DRM_IOCTL_MODE_SETCRTC => Self::DrmModeSetCrtc(ptr as *mut DrmModeCrtc),
            DRM_IOCTL_MODE_CURSOR => Self::DrmModeCursor(ptr as *mut DrmModeCursor),
            DRM_IOCTL_MODE_GETGAMMA => Self::DrmModeGetGamma(ptr as *mut DrmModeCrtcLut),
            DRM_IOCTL_MODE_SETGAMMA => Self::DrmModeSetGamma(ptr as *mut DrmModeCrtcLut),
            DRM_IOCTL_MODE_GETENCODER => Self::DrmModeGetEncoder(ptr as *mut DrmModeGetEncoder),
            DRM_IOCTL_MODE_GETCONNECTOR => {
                Self::DrmModeGetConnector(ptr as *mut DrmModeGetConnector)
            }
            DRM_IOCTL_MODE_GETPROPERTY => Self::DrmModeGetProperty(ptr as *mut DrmModeGetProperty),
            DRM_IOCTL_MODE_OBJ_GETPROPERTIES => {
                Self::DrmModeObjGetProperties(ptr as *mut DrmModeObjGetProperties)
            }
            DRM_IOCTL_MODE_GETPLANERESOURCES => {
                Self::DrmModeGetPlaneResources(ptr as *mut DrmModeGetPlaneRes)
            }
            DRM_IOCTL_MODE_GETPLANE => Self::DrmModeGetPlane(ptr as *mut DrmModeGetPlane),
            DRM_IOCTL_MODE_LIST_LESSEES => Self::DrmModeListLessees(ptr as *mut DrmModeListLessees),
            DRM_IOCTL_MODE_ADDFB => Self::DrmModeAddFb(ptr as *mut DrmModeFbCmd),
            DRM_IOCTL_MODE_ADDFB2 => Self::DrmModeAddFb2(ptr as *mut DrmModeFbCmd2),
            DRM_IOCTL_MODE_CURSOR2 => Self::DrmModeCursor2(ptr as *mut DrmModeCursor2),
            DRM_IOCTL_MODE_RMFB => Self::DrmModeRemoveFb(ptr as *mut u32),
            DRM_IOCTL_MODE_PAGE_FLIP => Self::DrmModePageFlip(ptr as *mut DrmModeCrtcPageFlip),
            DRM_IOCTL_MODE_DIRTYFB => Self::DrmModeDirtyFb(ptr as *mut DrmModeFbDirtyCmd),
            DRM_IOCTL_MODE_CREATE_DUMB => Self::DrmModeCreateDumb(ptr as *mut DrmModeCreateDumb),
            DRM_IOCTL_MODE_MAP_DUMB => Self::DrmModeMapDumb(ptr as *mut DrmModeMapDumb),
            DRM_IOCTL_MODE_DESTROY_DUMB => Self::DrmModeDestroyDumb(ptr as *mut DrmModeDestroyDumb),
            DRM_IOCTL_GEM_CLOSE => Self::DrmGemClose(ptr as *mut DrmGemClose),
            DRM_IOCTL_PRIME_HANDLE_TO_FD => Self::DrmPrimeHandleToFd(ptr as *mut DrmPrimeHandle),
            DRM_IOCTL_PRIME_FD_TO_HANDLE => Self::DrmPrimeFdToHandle(ptr as *mut DrmPrimeHandle),
            0x4600 => Self::FbGetVariableScreenInfo(ptr as *mut FbVarScreeninfo),
            0x4601 => Self::FbPutVariableScreenInfo(ptr as *mut FbVarScreeninfo),
            0x4602 => Self::FbGetFixedScreenInfo(ptr as *mut FbFixScreeninfo),
            0x4604 => Self::FbGetColorMap(ptr as *mut FbCmap),
            0x4605 => Self::FbPutColorMap(ptr as *mut FbCmap),
            0x4606 => Self::FbPanDisplay(ptr as *mut FbVarScreeninfo),
            0x4611 => Self::FbBlank(ptr as u32),
            0x5401 => Self::LinuxTcGets(ptr as *mut LinuxTermios),
            0x5402..=0x5404 => Self::LinuxTcSets(ptr as *const LinuxTermios),
            0x540B => Self::LinuxTcFlush(ptr as u32),
            0x540D => Self::LinuxTiocnxcl,
            0x540E => Self::LinuxTiocsctty(ptr as u32),
            0x802C542A => Self::LinuxTcGets2(ptr as *mut LinuxTermios2),
            0x402C542B..=0x402C542D => Self::LinuxTcSets2(ptr as *const LinuxTermios2),
            0x540F => Self::LinuxTiocgPgrp(ptr as *mut i32),
            0x5422 => Self::LinuxTiocnotty,
            0x5410 => Self::LinuxTiocspgrp(ptr as *const i32),
            0x5411 => Self::LinuxTiocoutq(ptr as *mut i32),
            0x5413 => Self::LinuxTiocgwinsz(ptr as *mut LinuxWinsize),
            0x5414 => Self::LinuxTiocswinsz(ptr as *const LinuxWinsize),
            0x80045430 => Self::LinuxTiocgptn(ptr as *mut i32),
            0x40045431 => Self::LinuxTiocsptlck(ptr as *const i32),
            0x5441 => Self::LinuxTiocgptpeer(PtyPeerOpenRequest::new(ptr)?),
            0x5437 => Self::LinuxTiocvhangup,
            0x4B44 => Self::LinuxKdGetKeyboardMode(ptr as *mut u32),
            0x4B45 => Self::LinuxKdSetKeyboardMode(ptr as u32),
            0x4B33 => Self::LinuxKdGetKeyboardType(ptr as *mut u8),
            0x4B46 => Self::LinuxKdGetKeyboardEntry(ptr as *mut LinuxKbEntry),
            0x4B3B => Self::LinuxKdGetDisplayMode(ptr as *mut u32),
            0x4B3A => Self::LinuxKdSetDisplayMode(ptr as u32),
            0x4B4E => Self::LinuxKdSignalAccept(ptr as u32),
            0x5600 => Self::LinuxVtOpenQuery(ptr as *mut u32),
            0x5601 => Self::LinuxVtGetMode(ptr as *mut LinuxVtMode),
            0x5603 => Self::LinuxVtGetState(ptr as *mut LinuxVtStat),
            0x5602 => Self::LinuxVtSetMode(ptr as *const LinuxVtMode),
            0x5606 => Self::LinuxVtActivate(ptr as u32),
            0x5607 => Self::LinuxVtWaitActive(ptr as u32),
            0x5605 => Self::LinuxVtRelDisp(ptr as u32),
            request if evdev_raw_ioctl_op(request).is_some() => {
                let len = ioctl_size(request);
                match ioctl_nr(request) {
                    0x01 => Self::EvdevGetVersion(ptr as *mut i32),
                    0x02 => Self::EvdevGetId(ptr as *mut u8),
                    0x03 => Self::EvdevGetRepeat(ptr as *mut [u32; 2]),
                    0x06 => Self::EvdevGetName {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x07 => Self::EvdevGetPhys {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x08 => Self::EvdevGetUniq {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x09 => Self::EvdevGetProp {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x18 => Self::EvdevGetKey {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x19 => Self::EvdevGetLed {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x1a => Self::EvdevGetSnd {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x1b => Self::EvdevGetSw {
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x20..=0x3f => Self::EvdevGetBit {
                        event_type: (ioctl_nr(request) - 0x20) as u8,
                        ptr: ptr as *mut u8,
                        len,
                    },
                    0x90 => Self::EvdevGrab(ptr),
                    0x91 => Self::EvdevRevoke(ptr),
                    0xa0 => Self::EvdevSetClockId(ptr as *const i32),
                    _ => unreachable!(),
                }
            }
            request if drm_prime_raw_ioctl_op(request).is_some() => match ioctl_nr(request) {
                0 => Self::DmaBufSync(ptr as *mut DmaBufSync),
                2 => Self::DmaBufExportSyncFile(ptr as *mut DmaBufExportSyncFile),
                3 => Self::DmaBufImportSyncFile(ptr as *mut DmaBufImportSyncFile),
                _ => unreachable!(),
            },
            _ => Self::RawIoctl { request, arg: ptr },
        })
    }
}
