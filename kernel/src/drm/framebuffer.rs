use core::{
    cmp::min,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{
    drm::mode::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888},
    misc::framebuffer::{FRAME_BUFFER, FramebufferPixelFormat, framebuffer_set_user_controlled},
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    object::DRM_STATE,
    state::{DrmState, DumbBuffer, RegisteredFramebuffer},
};

static SCANOUT_DEBUG_PRESENTS: AtomicU32 = AtomicU32::new(0);

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
    let (framebuffer, dumb_buffer) = {
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
        (framebuffer, dumb_buffer)
    };

    log_scanout_buffer_sample("set", &framebuffer, &dumb_buffer)?;
    if !dumb_buffer.scanout_backed {
        // TODO: This is still a legacy compatibility bridge over the Limine
        // framebuffer, not a real KMS scanout implementation.
        blit_dumb_buffer_to_scanout(&dumb_buffer, &framebuffer)?;
    }
    framebuffer_set_user_controlled(true);
    Ok(())
}

pub(super) fn blit_dumb_buffer_to_scanout(
    dumb_buffer: &DumbBuffer,
    framebuffer: &RegisteredFramebuffer,
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
    let (sampled, non_black, checksum) = {
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

        sample_scanout(dst)
    };

    canvas.present_user_controlled();
    log_scanout_debug_sample("blit", framebuffer, sampled, non_black, checksum);
    Ok(())
}

fn sample_scanout(scanout: &[u8]) -> (usize, usize, u32) {
    let sample_stride = 97usize;
    let sample_pixels = scanout.chunks_exact(4).step_by(sample_stride);
    let mut sampled = 0usize;
    let mut non_black = 0usize;
    let mut checksum = 0u32;

    for pixel in sample_pixels {
        sampled += 1;
        let blue = pixel[0];
        let green = pixel[1];
        let red = pixel[2];
        checksum = checksum.wrapping_mul(16777619) ^ u32::from(red);
        checksum = checksum.wrapping_mul(16777619) ^ u32::from(green);
        checksum = checksum.wrapping_mul(16777619) ^ u32::from(blue);
        if red != 0 || green != 0 || blue != 0 {
            non_black += 1;
        }
    }

    (sampled, non_black, checksum)
}

fn log_scanout_buffer_sample(
    phase: &str,
    framebuffer: &RegisteredFramebuffer,
    dumb_buffer: &DumbBuffer,
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
    let (sampled, non_black, checksum) = sample_scanout(src);
    log_scanout_debug_sample(phase, framebuffer, sampled, non_black, checksum);
    Ok(())
}

fn log_scanout_debug_sample(
    phase: &str,
    framebuffer: &RegisteredFramebuffer,
    sampled: usize,
    non_black: usize,
    checksum: u32,
) {
    let present_index = SCANOUT_DEBUG_PRESENTS.fetch_add(1, Ordering::Relaxed);
    if present_index >= 12 {
        return;
    }

    crate::s_println!(
        "drm scanout sample phase={} present={} fb={} {}x{} sampled={} non_black={} checksum={:#010x}",
        phase,
        present_index,
        framebuffer.fb_id,
        framebuffer.width,
        framebuffer.height,
        sampled,
        non_black,
        checksum
    );
}
