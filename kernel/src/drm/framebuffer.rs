use core::{
    cmp::min,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{
    drm::mode::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888},
    misc::time::{NANOSECONDS_PER_MILLISECOND, Time},
    misc::framebuffer::{FRAME_BUFFER, FramebufferPixelFormat, framebuffer_set_user_controlled},
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    object::DRM_STATE,
    state::{CursorState, DrmState, DumbBuffer, RegisteredFramebuffer},
};

static DRM_DEBUG_SAMPLES: AtomicU32 = AtomicU32::new(0);
static LAST_LEGACY_SCANOUT_REFRESH_NS: AtomicU32 = AtomicU32::new(0);
static LEGACY_REFRESH_DEBUG_SAMPLES: AtomicU32 = AtomicU32::new(0);

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
        // TODO: This is still a legacy compatibility bridge over the Limine
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

pub(crate) fn refresh_legacy_scanout_tick() {
    const REFRESH_INTERVAL_NS: u64 = 16 * NANOSECONDS_PER_MILLISECOND;

    let now_ns = Time::since_boot().as_nanoseconds();
    let now_ms = u32::try_from(now_ns / NANOSECONDS_PER_MILLISECOND).unwrap_or(u32::MAX);
    let last_ms = LAST_LEGACY_SCANOUT_REFRESH_NS.load(Ordering::Relaxed);
    if now_ms.wrapping_sub(last_ms) < u32::try_from(REFRESH_INTERVAL_NS / NANOSECONDS_PER_MILLISECOND).unwrap_or(16) {
        return;
    }

    let should_refresh = {
        let state = DRM_STATE.lock();
        state.current_fb_id.is_some_and(|fb_id| {
            state
                .framebuffers
                .get(&fb_id)
                .and_then(|framebuffer| state.dumb_buffers.get(&framebuffer.handle))
                .is_some_and(|buffer| !buffer.scanout_backed)
        })
    };

    if !should_refresh {
        return;
    }

    LAST_LEGACY_SCANOUT_REFRESH_NS.store(now_ms, Ordering::Relaxed);
    if LEGACY_REFRESH_DEBUG_SAMPLES.fetch_add(1, Ordering::Relaxed) < 16 {
        let _ = refresh_current_scanout().and_then(|()| {
            let current_fb_id = {
                let state = DRM_STATE.lock();
                state.current_fb_id
            };
            if let Some(fb_id) = current_fb_id {
                log_framebuffer_sample("legacy-refresh", fb_id)?;
            }
            Ok(())
        });
        return;
    }
    let _ = refresh_current_scanout();
}

pub(super) fn log_framebuffer_sample(phase: &str, fb_id: u32) -> ObjectResult<()> {
    let sample_index = DRM_DEBUG_SAMPLES.fetch_add(1, Ordering::Relaxed);
    if sample_index >= 32 {
        return Ok(());
    }

    let (framebuffer, dumb_buffer) = {
        let state = DRM_STATE.lock();
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
        (framebuffer, dumb_buffer)
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
    let (sampled, non_zero, checksum) = sample_buffer(src);
    crate::s_println!(
        "drm debug phase={} idx={} fb={} {}x{} pitch={} scanout_backed={} sampled={} non_zero={} checksum={:#010x}",
        phase,
        sample_index,
        framebuffer.fb_id,
        framebuffer.width,
        framebuffer.height,
        framebuffer.pitch,
        dumb_buffer.scanout_backed,
        sampled,
        non_zero,
        checksum
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

    if dst_bytes_per_pixel < 3 || src_pitch < width * 4 {
        return Err(ObjectError::InvalidArguments);
    }

    let clear_staging = width != canvas.info.width || height != canvas.info.height;
    let dst = canvas.user_controlled_buffer_mut();
    if clear_staging {
        dst.fill(0);
    }

    for y in 0..height {
        let src_row_start = y
            .checked_mul(src_pitch)
            .ok_or(ObjectError::InvalidArguments)?;
        let src_row_end = src_row_start
            .checked_add(width * 4)
            .ok_or(ObjectError::InvalidArguments)?;
        if src_row_end > src.len() {
            return Err(ObjectError::InvalidArguments);
        }

        let dst_row_start = y
            .checked_mul(dst_stride_bytes)
            .ok_or(ObjectError::InvalidArguments)?;
        let dst_row_end = dst_row_start
            .checked_add(width * dst_bytes_per_pixel)
            .ok_or(ObjectError::InvalidArguments)?;
        if dst_row_end > dst.len() {
            return Err(ObjectError::InvalidArguments);
        }

        let src_row = &src[src_row_start..src_row_end];
        let dst_row = &mut dst[dst_row_start..dst_row_end];

        for x in 0..width {
            let src_px = &src_row[x * 4..x * 4 + 4];
            let dst_px = &mut dst_row[x * dst_bytes_per_pixel..(x + 1) * dst_bytes_per_pixel];

            let blue = src_px[0];
            let green = src_px[1];
            let red = src_px[2];
            let alpha = if framebuffer.pixel_format == DRM_FORMAT_ARGB8888 {
                src_px[3]
            } else {
                0xff
            };

            match dst_pixel_format {
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

            if dst_bytes_per_pixel >= 4 {
                dst_px[3] = alpha;
            }
        }
    }

    if let Some(cursor) = cursor {
        overlay_cursor(&mut canvas, cursor)?;
    }

    canvas.present_user_controlled();
    Ok(())
}

fn overlay_cursor(
    canvas: &mut crate::misc::framebuffer::Canvas,
    cursor: &CursorState,
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

            let src_px_start = cursor_x
                .checked_mul(4)
                .ok_or(ObjectError::InvalidArguments)?;
            let src_px = &src_row[src_px_start..src_px_start + 4];
            let alpha = u16::from(src_px[3]);
            if alpha == 0 {
                continue;
            }

            let dst_px_start = usize::try_from(dst_y)
                .map_err(|_| ObjectError::InvalidArguments)?
                .checked_mul(dst_stride_bytes)
                .and_then(|row| {
                    usize::try_from(dst_x)
                        .ok()
                        .and_then(|x| x.checked_mul(dst_bytes_per_pixel))
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

fn sample_buffer(buffer: &[u8]) -> (usize, usize, u32) {
    let mut sampled = 0usize;
    let mut non_zero = 0usize;
    let mut checksum = 0u32;

    for chunk in buffer.chunks_exact(4).step_by(97) {
        sampled += 1;
        for byte in &chunk[..3] {
            checksum = checksum.wrapping_mul(16777619) ^ u32::from(*byte);
            if *byte != 0 {
                non_zero += 1;
            }
        }
    }

    (sampled, non_zero, checksum)
}
