use crate::{
    drm::{
        card::CRTC0_ID,
        mode::{DRM_MODE_CURSOR_BO, DRM_MODE_CURSOR_FLAGS, DRM_MODE_CURSOR_MOVE},
        mode_types::{DrmModeCursor, DrmModeCursor2},
    },
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    framebuffer::{refresh_current_scanout, refresh_cursor_move},
    object::DRM_STATE,
    state::CursorState,
    user::read_user,
};

pub(super) fn handle_mode_cursor(ptr: *mut DrmModeCursor) -> ObjectResult<isize> {
    let cursor = read_user(ptr)?;
    apply_cursor_update(CursorRequest {
        flags: cursor.flags,
        crtc_id: cursor.crtc_id,
        x: cursor.x,
        y: cursor.y,
        width: cursor.width,
        height: cursor.height,
        handle: cursor.handle,
        hot_x: 0,
        hot_y: 0,
    })
}

pub(super) fn handle_mode_cursor2(ptr: *mut DrmModeCursor2) -> ObjectResult<isize> {
    let cursor = read_user(ptr)?;
    apply_cursor_update(CursorRequest {
        flags: cursor.flags,
        crtc_id: cursor.crtc_id,
        x: cursor.x,
        y: cursor.y,
        width: cursor.width,
        height: cursor.height,
        handle: cursor.handle,
        hot_x: cursor.hot_x,
        hot_y: cursor.hot_y,
    })
}

struct CursorRequest {
    flags: u32,
    crtc_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    handle: u32,
    hot_x: i32,
    hot_y: i32,
}

fn apply_cursor_update(request: CursorRequest) -> ObjectResult<isize> {
    if request.flags & !DRM_MODE_CURSOR_FLAGS != 0 {
        return Err(ObjectError::InvalidArguments);
    }
    if request.crtc_id != 0 && request.crtc_id != CRTC0_ID {
        return Err(ObjectError::InvalidArguments);
    }
    if request.hot_x < 0 || request.hot_y < 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let mut moved_cursor = None;
    {
        let mut state = DRM_STATE.lock();
        if request.flags & DRM_MODE_CURSOR_BO != 0 {
            if request.handle == 0 {
                state.cursor = None;
            } else {
                let buffer = state.get_user_handle(request.handle)?;
                let min_bytes = u64::from(request.width)
                    .checked_mul(u64::from(request.height))
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or(ObjectError::InvalidArguments)?;
                if request.width == 0
                    || request.height == 0
                    || request.width > buffer.width
                    || request.height > buffer.height
                    || buffer.bpp < 32
                    || min_bytes > buffer.size
                    || u32::try_from(request.hot_x).map_err(|_| ObjectError::InvalidArguments)?
                        >= request.width
                    || u32::try_from(request.hot_y).map_err(|_| ObjectError::InvalidArguments)?
                        >= request.height
                {
                    return Err(ObjectError::InvalidArguments);
                }
                state.cursor = Some(CursorState {
                    handle: request.handle,
                    width: request.width,
                    height: request.height,
                    x: request.x,
                    y: request.y,
                    hot_x: request.hot_x,
                    hot_y: request.hot_y,
                });
            }
        }

        if request.flags & DRM_MODE_CURSOR_MOVE != 0
            && let Some(cursor) = state.cursor.as_mut()
        {
            let previous_cursor = cursor.clone();
            cursor.x = request.x;
            cursor.y = request.y;
            moved_cursor = Some((previous_cursor, cursor.clone()));
        }
    }

    if request.flags == DRM_MODE_CURSOR_MOVE
        && let Some((previous_cursor, current_cursor)) = moved_cursor
    {
        refresh_cursor_move(&previous_cursor, &current_cursor)?;
        return Ok(0);
    }

    refresh_current_scanout()?;
    Ok(0)
}
