use crate::{
    drm::{
        card::CRTC0_ID,
        client::{
            DRIVER_DATE, DRIVER_DESC, DRIVER_NAME, DRM_CAP_DUMB_BUFFER, DRM_CAP_DUMB_PREFER_SHADOW,
            DRM_CAP_DUMB_PREFERRED_DEPTH, DRM_CAP_TIMESTAMP_MONOTONIC, DRM_CLIENT_CAP_ASPECT_RATIO,
            DRM_CLIENT_CAP_ATOMIC, DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, DRM_CLIENT_CAP_STEREO_3D,
            DRM_CLIENT_CAP_UNIVERSAL_PLANES, DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, DRM_EVENT_VBLANK,
            DRM_VBLANK_EVENT, DRM_VBLANK_FLAGS_MASK, DRM_VBLANK_FLIP, DRM_VBLANK_SIGNAL,
            DRM_VBLANK_TYPES_MASK,
        },
    },
    memory::user_safe,
    misc::framebuffer::framebuffer_set_user_controlled,
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    events::{make_vblank_reply, queue_vblank_event},
    object::DRM_STATE,
    user::{copy_c_string, read_user},
};

pub(super) fn handle_version(ptr: *mut crate::drm::client::DrmVersion) -> ObjectResult<isize> {
    let mut version = read_user(ptr)?;
    version.version_major = 1;
    version.version_minor = 0;
    version.version_patchlevel = 0;
    copy_c_string(version.name, version.name_len, DRIVER_NAME)?;
    copy_c_string(version.date, version.date_len, DRIVER_DATE)?;
    copy_c_string(version.desc, version.desc_len, DRIVER_DESC)?;
    version.name_len = DRIVER_NAME.len();
    version.date_len = DRIVER_DATE.len();
    version.desc_len = DRIVER_DESC.len();
    user_safe::write(ptr, &version).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_get_cap(ptr: *mut crate::drm::client::DrmGetCap) -> ObjectResult<isize> {
    let mut cap = read_user(ptr)?;
    cap.value = match cap.capability {
        DRM_CAP_DUMB_BUFFER => 1,
        DRM_CAP_DUMB_PREFERRED_DEPTH => 32,
        DRM_CAP_DUMB_PREFER_SHADOW => 0,
        DRM_CAP_TIMESTAMP_MONOTONIC => 1,
        _ => return Err(ObjectError::InvalidArguments),
    };
    user_safe::write(ptr, &cap).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_wait_vblank(
    ptr: *mut crate::drm::client::DrmWaitVblank,
) -> ObjectResult<isize> {
    let mut wait = read_user(ptr)?;
    let request = unsafe { wait.request };
    let flags = request.type_ & DRM_VBLANK_FLAGS_MASK;
    if request.type_ & !(DRM_VBLANK_TYPES_MASK | DRM_VBLANK_FLAGS_MASK) != 0
        || flags & (DRM_VBLANK_SIGNAL | DRM_VBLANK_FLIP) != 0
    {
        return Err(ObjectError::InvalidArguments);
    }

    let reply = make_vblank_reply(request.type_, request.sequence);
    if request.type_ & DRM_VBLANK_EVENT != 0 {
        queue_vblank_event(request.signal, CRTC0_ID, DRM_EVENT_VBLANK, reply.sequence);
    }
    wait.reply = reply;
    user_safe::write(ptr, &wait).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_set_client_cap(
    ptr: *mut crate::drm::client::DrmSetClientCap,
) -> ObjectResult<isize> {
    let cap = read_user(ptr)?;
    match (cap.capability, cap.value) {
        (DRM_CLIENT_CAP_STEREO_3D, 0 | 1)
        | (DRM_CLIENT_CAP_UNIVERSAL_PLANES, 0 | 1)
        | (DRM_CLIENT_CAP_ASPECT_RATIO, 0 | 1) => Ok(0),
        (DRM_CLIENT_CAP_ATOMIC, 0)
        | (DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, 0)
        | (DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, 0) => Ok(0),
        (DRM_CLIENT_CAP_ATOMIC, _)
        | (DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, _)
        | (DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, _) => Err(ObjectError::Unimplemented),
        _ => Err(ObjectError::InvalidArguments),
    }
}

pub(super) fn handle_set_master() -> ObjectResult<isize> {
    Ok(0)
}

pub(super) fn handle_drop_master() -> ObjectResult<isize> {
    framebuffer_set_user_controlled(false);
    DRM_STATE.lock().current_fb_id = None;
    Ok(0)
}
