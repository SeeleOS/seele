use crate::{
    drm::{
        card::CRTC0_ID,
        client::DrmGemClose,
        mode::{
            DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_MODE_FB_MODIFIERS,
            DRM_MODE_PAGE_FLIP_ASYNC, DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_TARGET,
        },
        mode_types::{DrmModeCreateDumb, DrmModeFbCmd2},
    },
    memory::user_safe,
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    events::queue_page_flip_event,
    framebuffer::{build_framebuffer, scanout_framebuffer_id},
    object::DRM_STATE,
    user::read_user,
};

pub(super) fn handle_mode_add_fb(
    ptr: *mut crate::drm::mode_types::DrmModeFbCmd,
) -> ObjectResult<isize> {
    let mut fb = read_user(ptr)?;
    let pixel_format = match (fb.bpp, fb.depth) {
        (32, 24) => DRM_FORMAT_XRGB8888,
        (32, 32) => DRM_FORMAT_ARGB8888,
        _ => return Err(ObjectError::InvalidArguments),
    };
    let registered = build_framebuffer(
        &DRM_STATE.lock(),
        fb.handle,
        fb.width,
        fb.height,
        fb.pitch,
        0,
        pixel_format,
    )?;
    let mut state = DRM_STATE.lock();
    let fb_id = state.next_fb_id()?;
    let mut registered = registered;
    registered.fb_id = fb_id;
    state.register_framebuffer(registered);
    fb.fb_id = fb_id;
    user_safe::write(ptr, &fb).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_add_fb2(ptr: *mut DrmModeFbCmd2) -> ObjectResult<isize> {
    let mut fb = read_user(ptr)?;
    if fb.flags & !DRM_MODE_FB_MODIFIERS != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if !matches!(fb.pixel_format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888) {
        return Err(ObjectError::InvalidArguments);
    }
    if fb.handles[1..].iter().any(|&handle| handle != 0)
        || fb.pitches[1..].iter().any(|&pitch| pitch != 0)
        || fb.offsets[1..].iter().any(|&offset| offset != 0)
        || fb.modifier[1..].iter().any(|&modifier| modifier != 0)
    {
        return Err(ObjectError::InvalidArguments);
    }
    if fb.flags & DRM_MODE_FB_MODIFIERS != 0 && fb.modifier[0] != 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let registered = build_framebuffer(
        &DRM_STATE.lock(),
        fb.handles[0],
        fb.width,
        fb.height,
        fb.pitches[0],
        fb.offsets[0],
        fb.pixel_format,
    )?;
    let mut state = DRM_STATE.lock();
    let fb_id = state.next_fb_id()?;
    let mut registered = registered;
    registered.fb_id = fb_id;
    state.register_framebuffer(registered);
    fb.fb_id = fb_id;
    user_safe::write(ptr, &fb).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_remove_fb(ptr: *mut u32) -> ObjectResult<isize> {
    let fb_id = read_user(ptr)?;
    let mut state = DRM_STATE.lock();
    state.remove_framebuffer(fb_id)?;
    if state.current_fb_id.is_none() {
        crate::misc::framebuffer::framebuffer_set_user_controlled(false);
    }
    Ok(0)
}

pub(super) fn handle_mode_page_flip(
    ptr: *mut crate::drm::mode_types::DrmModeCrtcPageFlip,
) -> ObjectResult<isize> {
    let flip = read_user(ptr)?;
    if flip.crtc_id != CRTC0_ID || flip.reserved != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if flip.flags & DRM_MODE_PAGE_FLIP_ASYNC != 0 || flip.flags & DRM_MODE_PAGE_FLIP_TARGET != 0 {
        return Err(ObjectError::Unimplemented);
    }
    if flip.flags & !DRM_MODE_PAGE_FLIP_EVENT != 0 {
        return Err(ObjectError::InvalidArguments);
    }

    scanout_framebuffer_id(flip.fb_id)?;
    if flip.flags & DRM_MODE_PAGE_FLIP_EVENT != 0 {
        queue_page_flip_event(flip.user_data, flip.crtc_id);
    }
    Ok(0)
}

pub(super) fn handle_mode_create_dumb(ptr: *mut DrmModeCreateDumb) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    DRM_STATE.lock().create_dumb_buffer(&mut request)?;
    user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_map_dumb(
    ptr: *mut crate::drm::mode_types::DrmModeMapDumb,
) -> ObjectResult<isize> {
    let mut request = read_user(ptr)?;
    let state = DRM_STATE.lock();
    let buffer = state.get_user_handle(request.handle)?;
    request.offset = buffer.map_offset;
    user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_destroy_dumb(
    ptr: *mut crate::drm::mode_types::DrmModeDestroyDumb,
) -> ObjectResult<isize> {
    let request = read_user(ptr)?;
    DRM_STATE.lock().close_dumb_handle(request.handle)?;
    Ok(0)
}

pub(super) fn handle_gem_close(ptr: *mut DrmGemClose) -> ObjectResult<isize> {
    let request: DrmGemClose = read_user(ptr)?;
    DRM_STATE.lock().close_dumb_handle(request.handle)?;
    Ok(0)
}
