use alloc::{sync::Arc, vec::Vec};
use x86_64::{
    PhysAddr,
    structures::paging::{PageTableFlags, PhysFrame, Size4KiB},
};

use crate::{
    memory::{
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    object::{error::ObjectError, misc::ObjectResult},
};

use super::state::{DrmState, DumbBuffer};

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
        let mut frames = Vec::with_capacity(pages);
        for page_index in 0..pages {
            let frame_addr = start_frame.start_address().as_u64() + (page_index as u64 * 4096);
            frames.push(PhysFrame::containing_address(PhysAddr::new(frame_addr)));
        }
        let frames = Arc::<[PhysFrame<Size4KiB>]>::from(frames);
        let shared_flags = PageTableFlags::empty();
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
                frames,
                pages,
                kernel_addr,
                shared_flags,
                user_handle_open: true,
                framebuffer_refs: 0,
                scanout_backed: false,
            },
        );

        request.handle = handle;
        request.pitch = pitch;
        request.size = size;
        if let Some((pid, comm)) = super::user::current_debug_process() {
            let buffer = self
                .dumb_buffers
                .get(&handle)
                .expect("new dumb buffer must exist");
            crate::s_println!(
                "drm create_dumb comm={} pid={} handle={} size={}x{} pitch={} bytes={:#x} pages={} map_offset={:#x} start_frame={:#x} kernel_addr={:#x} shared_flags={:#x} scanout_backed={}",
                comm,
                pid,
                handle,
                request.width,
                request.height,
                pitch,
                size,
                pages,
                buffer.map_offset,
                buffer.start_frame_addr(),
                buffer.kernel_addr,
                buffer.shared_flags.bits(),
                buffer.scanout_backed
            );
        }
        Ok(())
    }
}
