use alloc::collections::BTreeMap;

use x86_64::{
    PhysAddr,
    structures::paging::{PageTableFlags, PhysFrame, Size4KiB},
};

use crate::object::{error::ObjectError, misc::ObjectResult};

use super::object::DRM_BUFFER_OFFSET_BASE;

#[derive(Debug)]
pub(super) struct DrmState {
    pub(super) next_handle: u32,
    next_fb_id: u32,
    pub(super) next_map_offset: u64,
    pub(super) next_flip_sequence: u32,
    pub(super) dumb_buffers: BTreeMap<u32, DumbBuffer>,
    pub(super) framebuffers: BTreeMap<u32, RegisteredFramebuffer>,
    pub(super) current_fb_id: Option<u32>,
}

#[derive(Clone, Debug)]
pub(super) struct DumbBuffer {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bpp: u32,
    pub(super) size: u64,
    pub(super) map_offset: u64,
    pub(super) start_frame: PhysFrame<Size4KiB>,
    pub(super) pages: usize,
    pub(super) kernel_addr: u64,
    pub(super) shared_flags: PageTableFlags,
    pub(super) user_handle_open: bool,
    pub(super) framebuffer_refs: u32,
    pub(super) scanout_backed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct RegisteredFramebuffer {
    pub(super) fb_id: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch: u32,
    pub(super) offset: u32,
    pub(super) pixel_format: u32,
    pub(super) handle: u32,
}

impl DrmState {
    pub(super) const fn new() -> Self {
        Self {
            next_handle: 1,
            next_fb_id: 1,
            next_map_offset: DRM_BUFFER_OFFSET_BASE,
            next_flip_sequence: 1,
            dumb_buffers: BTreeMap::new(),
            framebuffers: BTreeMap::new(),
            current_fb_id: None,
        }
    }

    pub(super) fn register_framebuffer(&mut self, framebuffer: RegisteredFramebuffer) {
        let buffer = self
            .dumb_buffers
            .get_mut(&framebuffer.handle)
            .expect("framebuffer registration must reference an existing dumb buffer");
        buffer.framebuffer_refs = buffer
            .framebuffer_refs
            .checked_add(1)
            .expect("framebuffer refcount overflow");
        self.framebuffers.insert(framebuffer.fb_id, framebuffer);
    }

    pub(super) fn next_fb_id(&mut self) -> ObjectResult<u32> {
        let fb_id = self.next_fb_id;
        self.next_fb_id = self.next_fb_id.checked_add(1).ok_or(ObjectError::Other)?;
        Ok(fb_id)
    }

    pub(super) fn dumb_buffer_for_mapping(
        &self,
        offset: u64,
        pages: u64,
    ) -> ObjectResult<(usize, PhysFrame<Size4KiB>, PageTableFlags)> {
        for buffer in self.dumb_buffers.values() {
            let end_offset = buffer
                .map_offset
                .checked_add(buffer.aligned_size())
                .ok_or(ObjectError::InvalidArguments)?;
            if !(buffer.map_offset..end_offset).contains(&offset) {
                continue;
            }

            let byte_delta = offset - buffer.map_offset;
            if !byte_delta.is_multiple_of(4096) {
                return Err(ObjectError::InvalidArguments);
            }

            let page_delta =
                usize::try_from(byte_delta / 4096).map_err(|_| ObjectError::InvalidArguments)?;
            let requested_pages =
                usize::try_from(pages).map_err(|_| ObjectError::InvalidArguments)?;
            if requested_pages == 0 || page_delta + requested_pages > buffer.pages {
                return Err(ObjectError::InvalidArguments);
            }

            let start_addr =
                buffer.start_frame.start_address().as_u64() + (page_delta as u64 * 4096);
            return Ok((
                requested_pages,
                PhysFrame::containing_address(PhysAddr::new(start_addr)),
                buffer.shared_flags,
            ));
        }

        Err(ObjectError::InvalidArguments)
    }

    pub(super) fn get_user_handle(&self, handle: u32) -> ObjectResult<&DumbBuffer> {
        let buffer = self
            .dumb_buffers
            .get(&handle)
            .ok_or(ObjectError::InvalidArguments)?;
        if !buffer.user_handle_open {
            return Err(ObjectError::InvalidArguments);
        }
        Ok(buffer)
    }

    pub(super) fn close_dumb_handle(&mut self, handle: u32) -> ObjectResult<()> {
        let buffer = self
            .dumb_buffers
            .get_mut(&handle)
            .ok_or(ObjectError::InvalidArguments)?;
        if !buffer.user_handle_open {
            return Err(ObjectError::InvalidArguments);
        }
        buffer.user_handle_open = false;
        Ok(())
    }

    pub(super) fn remove_framebuffer(&mut self, fb_id: u32) -> ObjectResult<()> {
        let framebuffer = self
            .framebuffers
            .remove(&fb_id)
            .ok_or(ObjectError::InvalidArguments)?;
        let buffer = self
            .dumb_buffers
            .get_mut(&framebuffer.handle)
            .ok_or(ObjectError::InvalidArguments)?;
        buffer.framebuffer_refs = buffer
            .framebuffer_refs
            .checked_sub(1)
            .ok_or(ObjectError::InvalidArguments)?;
        if self.current_fb_id == Some(fb_id) {
            self.current_fb_id = None;
        }
        Ok(())
    }
}

impl DumbBuffer {
    pub(super) fn aligned_size(&self) -> u64 {
        self.size.div_ceil(4096) * 4096
    }

    pub(super) fn contains_scanout_range(
        &self,
        offset: u32,
        pitch: u32,
        width: u32,
        height: u32,
    ) -> bool {
        if width > self.width || height > self.height || self.bpp < 32 {
            return false;
        }

        let bytes_per_pixel = self.bpp.div_ceil(8);
        if pitch < width.saturating_mul(bytes_per_pixel) {
            return false;
        }

        let required = u64::from(offset)
            .saturating_add(u64::from(pitch).saturating_mul(u64::from(height.saturating_sub(1))))
            .saturating_add(u64::from(width) * u64::from(bytes_per_pixel));
        required <= self.size
    }
}
