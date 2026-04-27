use bitflags::bitflags;

use crate::{
    object::{FileFlags, ObjectResult, error::ObjectError},
    process::FdFlags,
};

use crate::drm::{
    client::{
        DRM_IOCTL_AUTH_MAGIC, DRM_IOCTL_DROP_MASTER, DRM_IOCTL_GEM_CLOSE, DRM_IOCTL_GET_CAP,
        DRM_IOCTL_GET_MAGIC, DRM_IOCTL_GET_UNIQUE, DRM_IOCTL_PRIME_HANDLE_TO_FD,
        DRM_IOCTL_SET_CLIENT_CAP, DRM_IOCTL_SET_MASTER, DRM_IOCTL_SET_UNIQUE, DRM_IOCTL_VERSION,
        DRM_IOCTL_WAIT_VBLANK, DrmAuth, DrmGemClose, DrmGetCap, DrmPrimeHandle, DrmSetClientCap,
        DrmUnique, DrmVersion, DrmWaitVblank,
    },
    mode::{
        DRM_IOCTL_MODE_ADDFB, DRM_IOCTL_MODE_ADDFB2, DRM_IOCTL_MODE_CREATE_DUMB,
        DRM_IOCTL_MODE_DESTROY_DUMB, DRM_IOCTL_MODE_DIRTYFB, DRM_IOCTL_MODE_GETCONNECTOR,
        DRM_IOCTL_MODE_GETCRTC, DRM_IOCTL_MODE_GETENCODER, DRM_IOCTL_MODE_GETGAMMA,
        DRM_IOCTL_MODE_GETPLANE, DRM_IOCTL_MODE_GETPLANERESOURCES, DRM_IOCTL_MODE_GETPROPERTY,
        DRM_IOCTL_MODE_GETRESOURCES, DRM_IOCTL_MODE_LIST_LESSEES, DRM_IOCTL_MODE_MAP_DUMB,
        DRM_IOCTL_MODE_OBJ_GETPROPERTIES, DRM_IOCTL_MODE_PAGE_FLIP, DRM_IOCTL_MODE_RMFB,
        DRM_IOCTL_MODE_SETCRTC, DRM_IOCTL_MODE_SETGAMMA,
    },
    mode_types::{
        DrmModeCardRes, DrmModeCreateDumb, DrmModeCrtc, DrmModeCrtcLut, DrmModeCrtcPageFlip,
        DrmModeDestroyDumb, DrmModeFbCmd, DrmModeFbCmd2, DrmModeFbDirtyCmd, DrmModeGetConnector,
        DrmModeGetEncoder, DrmModeGetPlane, DrmModeGetPlaneRes, DrmModeGetProperty,
        DrmModeListLessees, DrmModeMapDumb, DrmModeObjGetProperties,
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
    RawIoctl { request: u64, arg: u64 },
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
        let flags = PtyPeerOpenFlags::from_bits(flag_bits).ok_or(ObjectError::InvalidArguments)?;

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
            DRM_IOCTL_MODE_RMFB => Self::DrmModeRemoveFb(ptr as *mut u32),
            DRM_IOCTL_MODE_PAGE_FLIP => Self::DrmModePageFlip(ptr as *mut DrmModeCrtcPageFlip),
            DRM_IOCTL_MODE_DIRTYFB => Self::DrmModeDirtyFb(ptr as *mut DrmModeFbDirtyCmd),
            DRM_IOCTL_MODE_CREATE_DUMB => Self::DrmModeCreateDumb(ptr as *mut DrmModeCreateDumb),
            DRM_IOCTL_MODE_MAP_DUMB => Self::DrmModeMapDumb(ptr as *mut DrmModeMapDumb),
            DRM_IOCTL_MODE_DESTROY_DUMB => Self::DrmModeDestroyDumb(ptr as *mut DrmModeDestroyDumb),
            DRM_IOCTL_GEM_CLOSE => Self::DrmGemClose(ptr as *mut DrmGemClose),
            DRM_IOCTL_PRIME_HANDLE_TO_FD => Self::DrmPrimeHandleToFd(ptr as *mut DrmPrimeHandle),
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
            _ => Self::RawIoctl { request, arg: ptr },
        })
    }
}
