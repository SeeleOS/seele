use alloc::string::String;

use crate::misc::framebuffer::{FRAME_BUFFER, FramebufferInfo};

use super::mode_types::DrmModeModeInfo;

pub const DRM_IOCTL_MODE_GETRESOURCES: u64 = 0xc040_64a0;
pub const DRM_IOCTL_MODE_GETCRTC: u64 = 0xc068_64a1;
pub const DRM_IOCTL_MODE_SETCRTC: u64 = 0xc068_64a2;
pub const DRM_IOCTL_MODE_GETGAMMA: u64 = 0xc020_64a4;
pub const DRM_IOCTL_MODE_SETGAMMA: u64 = 0xc020_64a5;
pub const DRM_IOCTL_MODE_GETENCODER: u64 = 0xc014_64a6;
pub const DRM_IOCTL_MODE_GETCONNECTOR: u64 = 0xc050_64a7;
pub const DRM_IOCTL_MODE_GETPROPERTY: u64 = 0xc040_64aa;
pub const DRM_IOCTL_MODE_ADDFB: u64 = 0xc01c_64ae;
pub const DRM_IOCTL_MODE_RMFB: u64 = 0xc004_64af;
pub const DRM_IOCTL_MODE_PAGE_FLIP: u64 = 0xc018_64b0;
pub const DRM_IOCTL_MODE_DIRTYFB: u64 = 0xc018_64b1;
pub const DRM_IOCTL_MODE_CREATE_DUMB: u64 = 0xc020_64b2;
pub const DRM_IOCTL_MODE_MAP_DUMB: u64 = 0xc010_64b3;
pub const DRM_IOCTL_MODE_DESTROY_DUMB: u64 = 0xc004_64b4;
pub const DRM_IOCTL_MODE_GETPLANERESOURCES: u64 = 0xc010_64b5;
pub const DRM_IOCTL_MODE_GETPLANE: u64 = 0xc020_64b6;
pub const DRM_IOCTL_MODE_ADDFB2: u64 = 0xc068_64b8;
pub const DRM_IOCTL_MODE_OBJ_GETPROPERTIES: u64 = 0xc020_64b9;
pub const DRM_IOCTL_MODE_LIST_LESSEES: u64 = 0xc010_64c7;

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
pub const DRM_MODE_FB_DIRTY_ANNOTATE_COPY: u32 = 0x01;
pub const DRM_MODE_FB_DIRTY_ANNOTATE_FILL: u32 = 0x02;
pub const DRM_MODE_FB_DIRTY_FLAGS: u32 =
    DRM_MODE_FB_DIRTY_ANNOTATE_COPY | DRM_MODE_FB_DIRTY_ANNOTATE_FILL;
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
pub const DRM_MODE_PAGE_FLIP_ASYNC: u32 = 0x02;
pub const DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE: u32 = 0x04;
pub const DRM_MODE_PAGE_FLIP_TARGET_RELATIVE: u32 = 0x08;
pub const DRM_MODE_PAGE_FLIP_TARGET: u32 =
    DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE | DRM_MODE_PAGE_FLIP_TARGET_RELATIVE;
pub const DRM_PLANE_TYPE_OVERLAY: u64 = 0;
pub const DRM_PLANE_TYPE_PRIMARY: u64 = 1;
pub const DRM_PLANE_TYPE_CURSOR: u64 = 2;
pub const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
pub const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
pub const MODE_REFRESH_HZ: u32 = 60;
pub const MODE_GAMMA_LUT_SIZE: u32 = 256;

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
