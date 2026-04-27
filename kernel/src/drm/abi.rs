use alloc::string::String;

use crate::misc::framebuffer::{FRAME_BUFFER, FramebufferInfo};

pub const DRM_IOCTL_VERSION: u64 = 0xc040_6400;
pub const DRM_IOCTL_GET_CAP: u64 = 0xc010_640c;
pub const DRM_IOCTL_SET_CLIENT_CAP: u64 = 0x4010_640d;
pub const DRM_IOCTL_SET_MASTER: u64 = 0x0000_641e;
pub const DRM_IOCTL_DROP_MASTER: u64 = 0x0000_641f;
pub const DRM_IOCTL_MODE_GETRESOURCES: u64 = 0xc040_64a0;
pub const DRM_IOCTL_MODE_GETCRTC: u64 = 0xc068_64a1;
pub const DRM_IOCTL_MODE_SETCRTC: u64 = 0xc068_64a2;
pub const DRM_IOCTL_MODE_GETENCODER: u64 = 0xc014_64a6;
pub const DRM_IOCTL_MODE_GETCONNECTOR: u64 = 0xc050_64a7;
pub const DRM_IOCTL_MODE_GETPROPERTY: u64 = 0xc040_64aa;
pub const DRM_IOCTL_MODE_ADDFB: u64 = 0xc01c_64ae;
pub const DRM_IOCTL_MODE_RMFB: u64 = 0xc004_64af;
pub const DRM_IOCTL_MODE_PAGE_FLIP: u64 = 0xc018_64b0;
pub const DRM_IOCTL_GEM_CLOSE: u64 = 0x4008_6409;
pub const DRM_IOCTL_MODE_CREATE_DUMB: u64 = 0xc020_64b2;
pub const DRM_IOCTL_MODE_MAP_DUMB: u64 = 0xc010_64b3;
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u64 = 0xc004_64b4;
pub const DRM_IOCTL_MODE_OBJ_GETPROPERTIES: u64 = 0xc020_64b9;
pub const DRM_IOCTL_MODE_GETPLANERESOURCES: u64 = 0xc010_64b5;
pub const DRM_IOCTL_MODE_GETPLANE: u64 = 0xc020_64b6;
pub const DRM_IOCTL_MODE_ADDFB2: u64 = 0xc068_64b8;

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

pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;
pub const DRM_MODE_TYPE_DRIVER: u32 = 1 << 6;

pub const DRM_MODE_CONNECTED: u32 = 1;
pub const DRM_MODE_SUBPIXEL_UNKNOWN: u32 = 0;
pub const DRM_MODE_ENCODER_VIRTUAL: u32 = 5;
pub const DRM_MODE_CONNECTOR_VIRTUAL: u32 = 15;
pub const DRM_MODE_OBJECT_CRTC: u32 = 0xcccc_cccc;
pub const DRM_MODE_OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;
pub const DRM_MODE_OBJECT_ENCODER: u32 = 0xe0e0_e0e0;
pub const DRM_MODE_OBJECT_FB: u32 = 0xfbfb_fbfb;
pub const DRM_MODE_OBJECT_PLANE: u32 = 0xeeee_eeee;
pub const DRM_MODE_PROP_IMMUTABLE: u32 = 1 << 2;
pub const DRM_MODE_PROP_ENUM: u32 = 1 << 3;
pub const DRM_MODE_FB_MODIFIERS: u32 = 1 << 1;
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
pub const DRM_MODE_PAGE_FLIP_ASYNC: u32 = 0x02;
pub const DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE: u32 = 0x04;
pub const DRM_MODE_PAGE_FLIP_TARGET_RELATIVE: u32 = 0x08;
pub const DRM_MODE_PAGE_FLIP_TARGET: u32 =
    DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE | DRM_MODE_PAGE_FLIP_TARGET_RELATIVE;
pub const DRM_EVENT_FLIP_COMPLETE: u32 = 0x02;

pub const CARD0_ID: u32 = 0x1000;
pub const CRTC0_ID: u32 = 0x1001;
pub const ENCODER0_ID: u32 = 0x1002;
pub const CONNECTOR0_ID: u32 = 0x1003;
pub const PRIMARY_PLANE0_ID: u32 = 0x1004;
pub const PLANE_TYPE_PROP_ID: u32 = 0x1100;
pub const DRM_PLANE_TYPE_OVERLAY: u64 = 0;
pub const DRM_PLANE_TYPE_PRIMARY: u64 = 1;
pub const DRM_PLANE_TYPE_CURSOR: u64 = 2;
pub const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
pub const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;

pub const CARD0_MAJOR: u64 = 226;
pub const CARD0_MINOR: u64 = 0;
pub const CARD0_RDEV: u64 = (CARD0_MAJOR << 8) | CARD0_MINOR;

pub const DRIVER_NAME: &str = "seele";
pub const DRIVER_DATE: &str = "20260426";
pub const DRIVER_DESC: &str = "Seele DRM/KMS";
pub const MODE_REFRESH_HZ: u32 = 60;

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
pub struct DrmModeModeInfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeCardRes {
    pub fb_id_ptr: u64,
    pub crtc_id_ptr: u64,
    pub connector_id_ptr: u64,
    pub encoder_id_ptr: u64,
    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeCrtc {
    pub set_connectors_ptr: u64,
    pub count_connectors: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub gamma_size: u32,
    pub mode_valid: u32,
    pub mode: DrmModeModeInfo,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeGetEncoder {
    pub encoder_id: u32,
    pub encoder_type: u32,
    pub crtc_id: u32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeGetConnector {
    pub encoders_ptr: u64,
    pub modes_ptr: u64,
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_modes: u32,
    pub count_props: u32,
    pub count_encoders: u32,
    pub encoder_id: u32,
    pub connector_id: u32,
    pub connector_type: u32,
    pub connector_type_id: u32,
    pub connection: u32,
    pub mm_width: u32,
    pub mm_height: u32,
    pub subpixel: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeGetProperty {
    pub values_ptr: u64,
    pub enum_blob_ptr: u64,
    pub prop_id: u32,
    pub flags: u32,
    pub name: [u8; 32],
    pub count_values: u32,
    pub count_enum_blobs: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeFbCmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    pub handle: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeFbCmd2 {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub flags: u32,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],
    pub offsets: [u32; 4],
    pub modifier: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModePropertyEnum {
    pub value: u64,
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeObjGetProperties {
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_props: u32,
    pub obj_id: u32,
    pub obj_type: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeGetPlaneRes {
    pub plane_id_ptr: u64,
    pub count_planes: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeGetPlane {
    pub plane_id: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub possible_crtcs: u32,
    pub gamma_size: u32,
    pub count_format_types: u32,
    pub format_type_ptr: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeCrtcPageFlip {
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub reserved: u32,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeCreateDumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeMapDumb {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DrmModeDestroyDumb {
    pub handle: u32,
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

pub fn current_mode_info() -> DrmModeModeInfo {
    let fb = current_framebuffer_info();
    let hdisplay = u16::try_from(fb.width).unwrap_or(u16::MAX);
    let vdisplay = u16::try_from(fb.height).unwrap_or(u16::MAX);
    let hsync_start = hdisplay.saturating_add(40);
    let hsync_end = hsync_start.saturating_add(40);
    let htotal = hsync_end.saturating_add(80);
    let vsync_start = vdisplay.saturating_add(5);
    let vsync_end = vsync_start.saturating_add(5);
    let vtotal = vsync_end.saturating_add(20);
    let clock = u32::from(htotal)
        .saturating_mul(u32::from(vtotal))
        .saturating_mul(MODE_REFRESH_HZ)
        / 1000;
    let mut mode = DrmModeModeInfo {
        clock,
        hdisplay,
        hsync_start,
        hsync_end,
        htotal,
        hskew: 0,
        vdisplay,
        vsync_start,
        vsync_end,
        vtotal,
        vscan: 0,
        vrefresh: MODE_REFRESH_HZ,
        flags: 0,
        type_: DRM_MODE_TYPE_DRIVER | DRM_MODE_TYPE_PREFERRED,
        name: [0; 32],
    };
    let name = format_mode_name(fb.width, fb.height);
    for (dst, src) in mode.name.iter_mut().zip(name.bytes()) {
        *dst = src;
    }
    mode
}

pub fn current_framebuffer_info() -> FramebufferInfo {
    FRAME_BUFFER.get().unwrap().lock().fb_info()
}

fn format_mode_name(width: usize, height: usize) -> String {
    let mut name = String::new();
    use alloc::fmt::Write;
    let _ = write!(&mut name, "{width}x{height}");
    name
}
