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

        let mut framebuffer = FRAME_BUFFER.get().unwrap().lock();

        let fb_ptr = framebuffer.fb.as_mut_ptr();
        let fb_len = framebuffer.info.byte_len as u64;

        let map_offset = offset * 4096;
        let map_len = pages * 4096;

        if map_offset + map_len > fb_len {
            return Err(ObjectError::InvalidArguments);
        }

        let fb_start_virt = VirtAddr::new(fb_ptr as u64);
        let fb_start_phys = MAPPER
            .get()
            .unwrap()
            .lock()
            .translate_addr(fb_start_virt)
            .ok_or(ObjectError::InvalidArguments)?;
        let fb_page_offset = fb_start_phys.as_u64() & 0xfff;
        let fb_window_len = fb_page_offset + fb_len;
        if map_offset + map_len > fb_window_len {
            return Err(ObjectError::InvalidArguments);
        }

        let total_pages = (fb_window_len).div_ceil(4096);
        let mut frames = Vec::with_capacity(total_pages as usize);

        {
            let mapper = MAPPER.get().unwrap().lock();

            for page_index in 0..total_pages {
                let page_virt = VirtAddr::new(fb_start_virt.as_u64() - fb_page_offset)
                    + page_index * 4096;
                let page_phys = mapper
                    .translate_addr(page_virt)
                    .ok_or(ObjectError::InvalidArguments)?;
                frames.push(PhysFrame::containing_address(page_phys));
            }
        }

        let user_addr = with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                total_pages,
                permissions,
                Data::Shared {
                    frames: Arc::<[PhysFrame]>::from(frames),
                },
            )
        });

        framebuffer_set_user_controlled(true);

        Ok(user_addr + map_offset)
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
