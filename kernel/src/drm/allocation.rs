use x86_64::{
    VirtAddr,
    structures::paging::{PageTableFlags, PhysFrame, Size4KiB, Translate, mapper::TranslateResult},
};

use crate::{
    memory::{
        paging::{FRAME_ALLOCATOR, MAPPER},
        utils::apply_offset,
    },
    misc::framebuffer::FRAME_BUFFER,
    object::{error::ObjectError, misc::ObjectResult},
};

use super::state::{DrmState, DumbBuffer};
use crate::drm::mode::current_framebuffer_info;

impl DrmState {
    pub(super) fn create_dumb_buffer(
        &mut self,
        request: &mut crate::drm::mode_types::DrmModeCreateDumb,
    ) -> ObjectResult<()> {
        if request.width == 0 || request.height == 0 || request.bpp == 0 || request.flags != 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let bytes_per_pixel = request.bpp.div_ceil(8);
        let pitch = request
            .width
            .checked_mul(bytes_per_pixel)
            .ok_or(ObjectError::InvalidArguments)?;
        let size = u64::from(pitch)
            .checked_mul(u64::from(request.height))
            .ok_or(ObjectError::InvalidArguments)?;
        let pages =
            usize::try_from(size.div_ceil(4096)).map_err(|_| ObjectError::InvalidArguments)?;
        if pages == 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let (start_frame, kernel_addr, shared_flags, scanout_backed) =
            if let Some((start_frame, kernel_addr, shared_flags)) = self
                .try_allocate_scanout_backing(
                    request.width,
                    request.height,
                    request.bpp,
                    pitch,
                    size,
                    pages,
                )
            {
                (start_frame, kernel_addr, shared_flags, true)
            } else {
                let start_frame = FRAME_ALLOCATOR
                    .get()
                    .unwrap()
                    .lock()
                    .allocate_contiguous(pages)
                    .ok_or(ObjectError::Other)?;
                let kernel_addr = apply_offset(start_frame.start_address().as_u64());
                unsafe {
                    core::ptr::write_bytes(kernel_addr as *mut u8, 0, pages * 4096);
                }
                (start_frame, kernel_addr, PageTableFlags::empty(), false)
            };

        let handle = self.next_handle;
        self.next_handle = self.next_handle.checked_add(1).ok_or(ObjectError::Other)?;
        let map_offset = self.next_map_offset;
        self.next_map_offset = self
            .next_map_offset
            .checked_add((pages as u64) * 4096)
            .and_then(|next| next.checked_add(4096u64))
            .ok_or(ObjectError::Other)?;

        self.dumb_buffers.insert(
            handle,
            DumbBuffer {
                width: request.width,
                height: request.height,
                bpp: request.bpp,
                size,
                map_offset,
                start_frame,
                pages,
                kernel_addr,
                shared_flags,
                user_handle_open: true,
                framebuffer_refs: 0,
                scanout_backed,
            },
        );

        request.handle = handle;
        request.pitch = pitch;
        request.size = size;
        Ok(())
    }

    fn try_allocate_scanout_backing(
        &self,
        width: u32,
        height: u32,
        bpp: u32,
        pitch: u32,
        size: u64,
        pages: usize,
    ) -> Option<(PhysFrame<Size4KiB>, u64, PageTableFlags)> {
        if self
            .dumb_buffers
            .values()
            .any(|buffer| buffer.scanout_backed)
        {
            return None;
        }

        let fb_info = current_framebuffer_info();
        if bpp != 32
            || width != fb_info.width as u32
            || height != fb_info.height as u32
            || pitch != (fb_info.stride * fb_info.bytes_per_pixel) as u32
            || size > fb_info.byte_len as u64
        {
            return None;
        }

        let framebuffer = FRAME_BUFFER.get().unwrap().lock();
        let fb_addr = VirtAddr::new(framebuffer.fb.as_ptr() as u64);
        let mut shared_flags = PageTableFlags::NO_CACHE;
        let mapper = MAPPER.get().unwrap().lock();
        let phys = mapper.translate_addr(fb_addr)?;
        if phys.as_u64() & 0xfff != 0 {
            return None;
        }
        if let TranslateResult::Mapped { flags, .. } = mapper.translate(fb_addr) {
            shared_flags |= flags & (PageTableFlags::WRITE_THROUGH | PageTableFlags::NO_CACHE);
        }
        if (pages as u64) * 4096 > (fb_info.byte_len as u64).div_ceil(4096) * 4096 {
            return None;
        }

        Some((
            PhysFrame::containing_address(phys),
            apply_offset(phys.as_u64()),
            shared_flags,
        ))
    }
}
