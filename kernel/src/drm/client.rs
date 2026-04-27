pub const DRM_IOCTL_VERSION: u64 = 0xc040_6400;
pub const DRM_IOCTL_GET_CAP: u64 = 0xc010_640c;
pub const DRM_IOCTL_WAIT_VBLANK: u64 = 0xc018_643a;
pub const DRM_IOCTL_SET_CLIENT_CAP: u64 = 0x4010_640d;
pub const DRM_IOCTL_SET_MASTER: u64 = 0x0000_641e;
pub const DRM_IOCTL_DROP_MASTER: u64 = 0x0000_641f;
pub const DRM_IOCTL_GEM_CLOSE: u64 = 0x4008_6409;

pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
pub const DRM_CAP_DUMB_PREFERRED_DEPTH: u64 = 0x3;
pub const DRM_CAP_DUMB_PREFER_SHADOW: u64 = 0x4;
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;

pub const DRM_CLIENT_CAP_STEREO_3D: u64 = 1;
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
pub const DRM_CLIENT_CAP_ATOMIC: u64 = 3;
pub const DRM_CLIENT_CAP_ASPECT_RATIO: u64 = 4;
pub const DRM_CLIENT_CAP_WRITEBACK_CONNECTORS: u64 = 5;
pub const DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT: u64 = 6;

pub const DRM_EVENT_VBLANK: u32 = 0x01;
pub const DRM_EVENT_FLIP_COMPLETE: u32 = 0x02;
pub const DRM_VBLANK_ABSOLUTE: u32 = 0x0;
pub const DRM_VBLANK_RELATIVE: u32 = 0x1;
pub const DRM_VBLANK_EVENT: u32 = 0x0400_0000;
pub const DRM_VBLANK_FLIP: u32 = 0x0800_0000;
pub const DRM_VBLANK_NEXT_ON_MISS: u32 = 0x1000_0000;
pub const DRM_VBLANK_SECONDARY: u32 = 0x2000_0000;
pub const DRM_VBLANK_SIGNAL: u32 = 0x4000_0000;
pub const DRM_VBLANK_TYPES_MASK: u32 = DRM_VBLANK_ABSOLUTE | DRM_VBLANK_RELATIVE;
pub const DRM_VBLANK_FLAGS_MASK: u32 = DRM_VBLANK_EVENT
    | DRM_VBLANK_SIGNAL
    | DRM_VBLANK_SECONDARY
    | DRM_VBLANK_NEXT_ON_MISS
    | DRM_VBLANK_FLIP;

pub const DRIVER_NAME: &str = "seele";
pub const DRIVER_DATE: &str = "20260426";
pub const DRIVER_DESC: &str = "Seele DRM/KMS";

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmVersion {
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patchlevel: i32,
    pub name_len: usize,
    pub name: *mut u8,
    pub date_len: usize,
    pub date: *mut u8,
    pub desc_len: usize,
    pub desc: *mut u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmGetCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmSetClientCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmWaitVblankRequest {
    pub type_: u32,
    pub sequence: u32,
    pub signal: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmWaitVblankReply {
    pub type_: u32,
    pub sequence: u32,
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union DrmWaitVblank {
    pub request: DrmWaitVblankRequest,
    pub reply: DrmWaitVblankReply,
}

impl Default for DrmWaitVblank {
    fn default() -> Self {
        Self {
            request: DrmWaitVblankRequest::default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmGemClose {
    pub handle: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmEvent {
    pub type_: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmEventVblank {
    pub base: DrmEvent,
    pub user_data: u64,
    pub tv_sec: u32,
    pub tv_usec: u32,
    pub sequence: u32,
    pub crtc_id: u32,
}
