use core::{cmp::min, slice};

use crate::{
    drm::mode::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888},
    misc::framebuffer::{FRAME_BUFFER, FramebufferPixelFormat, framebuffer_set_user_controlled},
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    object::DRM_STATE,
    state::{CursorState, DrmState, DumbBuffer, RegisteredFramebuffer},
};

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy)]
struct ScanoutCopyContext {
    src_pixel_format: u32,
    dst_pixel_format: FramebufferPixelFormat,
    src_pitch: usize,
    dst_stride_bytes: usize,
    dst_bytes_per_pixel: usize,
}

pub(super) fn build_framebuffer(
    state: &DrmState,
    handle: u32,
    width: u32,
    height: u32,
    pitch: u32,
    offset: u32,
    pixel_format: u32,
) -> ObjectResult<RegisteredFramebuffer> {
    let buffer = state.get_user_handle(handle)?;
    if !buffer.contains_scanout_range(offset, pitch, width, height) {
        return Err(ObjectError::InvalidArguments);
    }
    if !matches!(pixel_format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888) {
        return Err(ObjectError::InvalidArguments);
    }
    Ok(RegisteredFramebuffer {
        fb_id: 0,
        width,
        height,
        pitch,
        offset,
        pixel_format,
        handle,
    })
}

pub(super) fn scanout_framebuffer_id(fb_id: u32) -> ObjectResult<()> {
    let (framebuffer, dumb_buffer, cursor) = {
        let mut state = DRM_STATE.lock();
        let framebuffer = state
            .framebuffers
            .get(&fb_id)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        let dumb_buffer = state
            .dumb_buffers
            .get(&framebuffer.handle)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        state.current_fb_id = Some(fb_id);
        (framebuffer, dumb_buffer, state.cursor.clone())
    };

    if !dumb_buffer.scanout_backed {
        // TODO: This is still a legacy compatibility bridge over the boot
        // framebuffer, not a real KMS scanout implementation.
        blit_dumb_buffer_to_scanout(&dumb_buffer, &framebuffer, cursor.as_ref())?;
    }
    framebuffer_set_user_controlled(true);
    Ok(())
}

pub(super) fn refresh_current_scanout() -> ObjectResult<()> {
    let current_fb_id = {
        let state = DRM_STATE.lock();
        state.current_fb_id
    };
    match current_fb_id {
        Some(fb_id) => scanout_framebuffer_id(fb_id),
        None => Ok(()),
    }
}

pub(super) fn refresh_cursor_move(
    previous_cursor: &CursorState,
    current_cursor: &CursorState,
) -> ObjectResult<()> {
    let (framebuffer, dumb_buffer, update_rect) = {
        let state = DRM_STATE.lock();
        let Some(current_fb_id) = state.current_fb_id else {
            return Ok(());
        };
        let framebuffer = state
            .framebuffers
            .get(&current_fb_id)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        let dumb_buffer = state
            .dumb_buffers
            .get(&framebuffer.handle)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        let update_rect = union_cursor_rects(
            &framebuffer,
            previous_cursor,
            current_cursor,
            dumb_buffer.width,
            dumb_buffer.height,
        );
        (framebuffer, dumb_buffer, update_rect)
    };

    let Some(update_rect) = update_rect else {
        return Ok(());
    };

    let src_start = dumb_buffer
        .kernel_addr
        .checked_add(u64::from(framebuffer.offset))
        .ok_or(ObjectError::InvalidArguments)?;
    let src_bytes = usize::try_from(
        dumb_buffer
            .size
            .checked_sub(u64::from(framebuffer.offset))
            .ok_or(ObjectError::InvalidArguments)?,
    )
    .map_err(|_| ObjectError::InvalidArguments)?;
    let src = unsafe { slice::from_raw_parts(src_start as *const u8, src_bytes) };

    let mut canvas = FRAME_BUFFER.get().unwrap().lock();
    redraw_scanout_region(&mut canvas, src, &framebuffer, update_rect)?;
    overlay_cursor_in_rect(&mut canvas, current_cursor, Some(update_rect))?;
    canvas.present_user_controlled_region(
        update_rect.x,
        update_rect.y,
        update_rect.width,
        update_rect.height,
    );
    Ok(())
}

pub(super) fn blit_dumb_buffer_to_scanout(
    dumb_buffer: &DumbBuffer,
    framebuffer: &RegisteredFramebuffer,
    cursor: Option<&CursorState>,
) -> ObjectResult<()> {
    let src_start = dumb_buffer
        .kernel_addr
        .checked_add(u64::from(framebuffer.offset))
        .ok_or(ObjectError::InvalidArguments)?;
    let src_bytes = usize::try_from(
        dumb_buffer
            .size
            .checked_sub(u64::from(framebuffer.offset))
            .ok_or(ObjectError::InvalidArguments)?,
    )
    .map_err(|_| ObjectError::InvalidArguments)?;
    let src = unsafe { slice::from_raw_parts(src_start as *const u8, src_bytes) };

    let mut canvas = FRAME_BUFFER.get().unwrap().lock();
    let width = min(framebuffer.width as usize, canvas.info.width);
    let height = min(framebuffer.height as usize, canvas.info.height);
    let dst_bytes_per_pixel = canvas.info.bytes_per_pixel;
    let dst_stride_bytes = canvas.info.stride * dst_bytes_per_pixel;
    let dst_pixel_format = canvas.info.pixel_format;
    let src_pitch = framebuffer.pitch as usize;
    let copy_context = ScanoutCopyContext {
        src_pixel_format: framebuffer.pixel_format,
        dst_pixel_format,
        src_pitch,
        dst_stride_bytes,
        dst_bytes_per_pixel,
    };

    if dst_bytes_per_pixel < 3 || src_pitch < width * 4 {
        return Err(ObjectError::InvalidArguments);
    }

    let clear_staging = width != canvas.info.width || height != canvas.info.height;
    let dst = canvas.user_controlled_buffer_mut();
    if clear_staging {
        dst.fill(0);
    }

    for y in 0..height {
        copy_scanout_row(src, dst, copy_context, 0, y, width)?;
    }

    if let Some(cursor) = cursor {
        overlay_cursor_in_rect(&mut canvas, cursor, None)?;
    }

    canvas.present_user_controlled();
    Ok(())
}

fn redraw_scanout_region(
    canvas: &mut crate::misc::framebuffer::Canvas,
    src: &[u8],
    framebuffer: &RegisteredFramebuffer,
    rect: Rect,
) -> ObjectResult<()> {
    let dst_bytes_per_pixel = canvas.info.bytes_per_pixel;
    let dst_stride_bytes = canvas.info.stride * dst_bytes_per_pixel;
    let dst_pixel_format = canvas.info.pixel_format;
    let src_pitch = framebuffer.pitch as usize;
    let copy_context = ScanoutCopyContext {
        src_pixel_format: framebuffer.pixel_format,
        dst_pixel_format,
        src_pitch,
        dst_stride_bytes,
        dst_bytes_per_pixel,
    };
    let dst = canvas.user_controlled_buffer_mut();

    for y in rect.y..rect.y.saturating_add(rect.height) {
        copy_scanout_row(src, dst, copy_context, rect.x, y, rect.width)?;
    }

    Ok(())
}

fn copy_scanout_row(
    src: &[u8],
    dst: &mut [u8],
    context: ScanoutCopyContext,
    x_start: usize,
    y: usize,
    width: usize,
) -> ObjectResult<()> {
    let src_row_start = y
        .checked_mul(context.src_pitch)
        .and_then(|offset| offset.checked_add(x_start * 4))
        .ok_or(ObjectError::InvalidArguments)?;
    let src_row_end = src_row_start
        .checked_add(width * 4)
        .ok_or(ObjectError::InvalidArguments)?;
    if src_row_end > src.len() {
        return Err(ObjectError::InvalidArguments);
    }

    let dst_row_start = y
        .checked_mul(context.dst_stride_bytes)
        .and_then(|offset| offset.checked_add(x_start * context.dst_bytes_per_pixel))
        .ok_or(ObjectError::InvalidArguments)?;
    let dst_row_end = dst_row_start
        .checked_add(width * context.dst_bytes_per_pixel)
        .ok_or(ObjectError::InvalidArguments)?;
    if dst_row_end > dst.len() {
        return Err(ObjectError::InvalidArguments);
    }

    let src_row = &src[src_row_start..src_row_end];
    let dst_row = &mut dst[dst_row_start..dst_row_end];

    for x in 0..width {
        let src_px = &src_row[x * 4..x * 4 + 4];
        let dst_px =
            &mut dst_row[x * context.dst_bytes_per_pixel..(x + 1) * context.dst_bytes_per_pixel];

        let blue = src_px[0];
        let green = src_px[1];
        let red = src_px[2];
        let alpha = if context.src_pixel_format == DRM_FORMAT_ARGB8888 {
            src_px[3]
        } else {
            0xff
        };

        match context.dst_pixel_format {
            FramebufferPixelFormat::Rgb => {
                dst_px[0] = red;
                dst_px[1] = green;
                dst_px[2] = blue;
            }
            FramebufferPixelFormat::Bgr => {
                dst_px[0] = blue;
                dst_px[1] = green;
                dst_px[2] = red;
            }
        }

        if context.dst_bytes_per_pixel >= 4 {
            dst_px[3] = alpha;
        }
    }

    Ok(())
}

fn overlay_cursor_in_rect(
    canvas: &mut crate::misc::framebuffer::Canvas,
    cursor: &CursorState,
    clip: Option<Rect>,
) -> ObjectResult<()> {
    let buffer = {
        let state = DRM_STATE.lock();
        state
            .dumb_buffers
            .get(&cursor.handle)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?
    };
    let src_bytes = usize::try_from(buffer.size).map_err(|_| ObjectError::InvalidArguments)?;
    let src = unsafe { slice::from_raw_parts(buffer.kernel_addr as *const u8, src_bytes) };
    let cursor_pitch = usize::try_from(cursor.width)
        .map_err(|_| ObjectError::InvalidArguments)?
        .checked_mul(4)
        .ok_or(ObjectError::InvalidArguments)?;
    let canvas_height = i32::try_from(canvas.info.height).unwrap_or(i32::MAX);
    let canvas_width = i32::try_from(canvas.info.width).unwrap_or(i32::MAX);
    let full_canvas_rect = Rect {
        x: 0,
        y: 0,
        width: canvas.info.width,
        height: canvas.info.height,
    };
    let dst_bytes_per_pixel = canvas.info.bytes_per_pixel;
    let dst_stride_bytes = canvas.info.stride * dst_bytes_per_pixel;
    let pixel_format = canvas.info.pixel_format;
    let dst = canvas.user_controlled_buffer_mut();
    let origin_x = cursor
        .x
        .checked_sub(cursor.hot_x)
        .ok_or(ObjectError::InvalidArguments)?;
    let origin_y = cursor
        .y
        .checked_sub(cursor.hot_y)
        .ok_or(ObjectError::InvalidArguments)?;
    let clip = clip.unwrap_or(full_canvas_rect);
    let clip_x_end = clip.x.saturating_add(clip.width);
    let clip_y_end = clip.y.saturating_add(clip.height);

    for cursor_y in 0..usize::try_from(cursor.height).map_err(|_| ObjectError::InvalidArguments)? {
        let dst_y =
            origin_y + i32::try_from(cursor_y).map_err(|_| ObjectError::InvalidArguments)?;
        if dst_y < 0 || dst_y >= canvas_height {
            continue;
        }
        let src_row_start = cursor_y
            .checked_mul(cursor_pitch)
            .ok_or(ObjectError::InvalidArguments)?;
        let src_row_end = src_row_start
            .checked_add(cursor_pitch)
            .ok_or(ObjectError::InvalidArguments)?;
        if src_row_end > src.len() {
            return Err(ObjectError::InvalidArguments);
        }
        let src_row = &src[src_row_start..src_row_end];

        for cursor_x in
            0..usize::try_from(cursor.width).map_err(|_| ObjectError::InvalidArguments)?
        {
            let dst_x =
                origin_x + i32::try_from(cursor_x).map_err(|_| ObjectError::InvalidArguments)?;
            if dst_x < 0 || dst_x >= canvas_width {
                continue;
            }
            let dst_x_usize = usize::try_from(dst_x).map_err(|_| ObjectError::InvalidArguments)?;
            let dst_y_usize = usize::try_from(dst_y).map_err(|_| ObjectError::InvalidArguments)?;
            if dst_x_usize < clip.x
                || dst_x_usize >= clip_x_end
                || dst_y_usize < clip.y
                || dst_y_usize >= clip_y_end
            {
                continue;
            }

            let src_px_start = cursor_x
                .checked_mul(4)
                .ok_or(ObjectError::InvalidArguments)?;
            let src_px = &src_row[src_px_start..src_px_start + 4];
            let alpha = u16::from(src_px[3]);
            if alpha == 0 {
                continue;
            }

            let dst_px_start = dst_y_usize
                .checked_mul(dst_stride_bytes)
                .and_then(|row| {
                    dst_x_usize
                        .checked_mul(dst_bytes_per_pixel)
                        .and_then(|col| row.checked_add(col))
                })
                .ok_or(ObjectError::InvalidArguments)?;
            let dst_px_end = dst_px_start
                .checked_add(dst_bytes_per_pixel)
                .ok_or(ObjectError::InvalidArguments)?;
            if dst_px_end > dst.len() {
                return Err(ObjectError::InvalidArguments);
            }
            let dst_px = &mut dst[dst_px_start..dst_px_end];

            let src_blue = u16::from(src_px[0]);
            let src_green = u16::from(src_px[1]);
            let src_red = u16::from(src_px[2]);
            let (mut dst_red, mut dst_green, mut dst_blue) = match pixel_format {
                FramebufferPixelFormat::Rgb => (
                    u16::from(dst_px[0]),
                    u16::from(dst_px[1]),
                    u16::from(dst_px[2]),
                ),
                FramebufferPixelFormat::Bgr => (
                    u16::from(dst_px[2]),
                    u16::from(dst_px[1]),
                    u16::from(dst_px[0]),
                ),
            };

            dst_red = ((src_red * alpha) + (dst_red * (255 - alpha))) / 255;
            dst_green = ((src_green * alpha) + (dst_green * (255 - alpha))) / 255;
            dst_blue = ((src_blue * alpha) + (dst_blue * (255 - alpha))) / 255;

            match pixel_format {
                FramebufferPixelFormat::Rgb => {
                    dst_px[0] = dst_red as u8;
                    dst_px[1] = dst_green as u8;
                    dst_px[2] = dst_blue as u8;
                }
                FramebufferPixelFormat::Bgr => {
                    dst_px[0] = dst_blue as u8;
                    dst_px[1] = dst_green as u8;
                    dst_px[2] = dst_red as u8;
                }
            }
            if dst_bytes_per_pixel >= 4 {
                dst_px[3] = 0xff;
            }
        }
    }

    Ok(())
}

fn cursor_rect(
    framebuffer: &RegisteredFramebuffer,
    cursor: &CursorState,
    buffer_width: u32,
    buffer_height: u32,
) -> Option<Rect> {
    let max_width = usize::try_from(framebuffer.width.min(buffer_width)).ok()?;
    let max_height = usize::try_from(framebuffer.height.min(buffer_height)).ok()?;
    let origin_x = cursor.x.checked_sub(cursor.hot_x)?;
    let origin_y = cursor.y.checked_sub(cursor.hot_y)?;
    let end_x = origin_x.checked_add(i32::try_from(cursor.width).ok()?)?;
    let end_y = origin_y.checked_add(i32::try_from(cursor.height).ok()?)?;

    let start_x = origin_x.max(0) as usize;
    let start_y = origin_y.max(0) as usize;
    let end_x = usize::try_from(end_x.max(0)).ok()?.min(max_width);
    let end_y = usize::try_from(end_y.max(0)).ok()?.min(max_height);
    if start_x >= end_x || start_y >= end_y {
        return None;
    }

    Some(Rect {
        x: start_x,
        y: start_y,
        width: end_x - start_x,
        height: end_y - start_y,
    })
}

fn union_cursor_rects(
    framebuffer: &RegisteredFramebuffer,
    previous_cursor: &CursorState,
    current_cursor: &CursorState,
    buffer_width: u32,
    buffer_height: u32,
) -> Option<Rect> {
    let previous = cursor_rect(framebuffer, previous_cursor, buffer_width, buffer_height);
    let current = cursor_rect(framebuffer, current_cursor, buffer_width, buffer_height);
    match (previous, current) {
        (Some(a), Some(b)) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let x_end = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
            let y_end =
                a.y.saturating_add(a.height)
                    .max(b.y.saturating_add(b.height));
            Some(Rect {
                x,
                y,
                width: x_end.saturating_sub(x),
                height: y_end.saturating_sub(y),
            })
        }
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (None, None) => None,
    }
}
