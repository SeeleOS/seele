use alloc::sync::Arc;
use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::Translate};

use crate::{
    impl_cast_function,
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        paging::MAPPER,
    },
    misc::{
        framebuffer::{FRAME_BUFFER, framebuffer_set_user_controlled},
        others::permissions_to_flags,
    },
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        traits::{Configuratable, MemoryMappable, Readable, Writable},
    },
    process::misc::with_current_process,
};

#[derive(Default, Debug)]
pub struct FramebufferObject {
    used_by_user: Mutex<bool>,
}

impl Object for FramebufferObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("mappable", MemoryMappable);
}

impl MemoryMappable for FramebufferObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        permissions: seele_sys::permission::Permissions,
    ) -> crate::object::misc::ObjectResult<VirtAddr> {
        use alloc::vec::Vec;
        use x86_64::structures::paging::PhysFrame;
        use x86_64::structures::paging::{PageTableFlags, mapper::TranslateResult};

        let mut framebuffer = FRAME_BUFFER.get().unwrap().lock();

        let fb_ptr = framebuffer.fb.as_mut_ptr();
        let fb_len = framebuffer.info.byte_len as u64;

        if pages == 0 || offset % 4096 != 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let map_offset = offset;
        let map_len = pages
            .checked_mul(4096)
            .ok_or(ObjectError::InvalidArguments)?;

        let fb_start_virt = VirtAddr::new(fb_ptr as u64);
        let fb_start_phys = MAPPER
            .get()
            .unwrap()
            .lock()
            .translate_addr(fb_start_virt)
            .ok_or(ObjectError::InvalidArguments)?;
        let fb_page_offset = fb_start_phys.as_u64() & 0xfff;
        let fb_window_len = fb_page_offset
            .checked_add(fb_len)
            .ok_or(ObjectError::InvalidArguments)?;
        let fb_window_len_aligned = fb_window_len.div_ceil(4096) * 4096;
        if map_offset
            .checked_add(map_len)
            .ok_or(ObjectError::InvalidArguments)?
            > fb_window_len_aligned
        {
            return Err(ObjectError::InvalidArguments);
        }

        let start_page_index = map_offset / 4096;
        let fb_base_virt = VirtAddr::new(fb_start_virt.as_u64() - fb_page_offset);
        let mut frames = Vec::with_capacity(pages as usize);
        let mut shared_flags = PageTableFlags::empty();

        {
            let mapper = MAPPER.get().unwrap().lock();

            if let TranslateResult::Mapped { flags, .. } = mapper.translate(fb_base_virt) {
                shared_flags = flags & (PageTableFlags::WRITE_THROUGH | PageTableFlags::NO_CACHE);
            }

            // Framebuffer memory is device memory, not normal DRAM. If the
            // bootloader left it cacheable, inheriting no cache bits would map
            // it as write-back in userspace, which can lead to corrupted or
            // stale scanout contents. Force uncached mappings for /dev/fb0.
            shared_flags |= PageTableFlags::NO_CACHE;

            for relative_page in 0..pages {
                let page_index = start_page_index + relative_page;
                let page_virt = fb_base_virt + page_index * 4096;
                let page_phys = mapper
                    .translate_addr(page_virt)
                    .ok_or(ObjectError::InvalidArguments)?;
                frames.push(PhysFrame::containing_address(page_phys));
            }
        }

        let user_addr = with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                permissions,
                Data::Shared {
                    frames: Arc::<[PhysFrame]>::from(frames),
                    flags: shared_flags,
                },
            )
        });

        framebuffer_set_user_controlled(true);

        // Xorg fbdev expects mmap(/dev/fb0) to return the page-aligned base.
        // It separately adds fix.smem_start's intra-page offset to compute
        // the first visible pixel address.
        Ok(user_addr)
    }
}

impl Configuratable for FramebufferObject {
    fn configure(
        &self,
        request: crate::object::config::ConfigurateRequest,
    ) -> crate::object::misc::ObjectResult<isize> {
        match request {
            ConfigurateRequest::GetFramebufferInfo(fb_info) => unsafe {
                fb_info.write(FRAME_BUFFER.get().unwrap().lock().fb_info());
            },
            ConfigurateRequest::FbTakeControl => {
                *self.used_by_user.lock() = true;
                framebuffer_set_user_controlled(true);
            }
            ConfigurateRequest::FbRelease => {
                *self.used_by_user.lock() = false;
                framebuffer_set_user_controlled(false);
            }
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
