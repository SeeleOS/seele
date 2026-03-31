use alloc::sync::Arc;
use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::Translate};

use crate::{
    impl_cast_function,
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        paging::MAPPER,
    },
    misc::{framebuffer::FRAME_BUFFER, others::permissions_to_flags},
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
        use x86_64::{VirtAddr, structures::paging::PhysFrame};

        let mut framebuffer = FRAME_BUFFER.get().unwrap().lock();

        let fb_ptr = framebuffer.fb.as_mut_ptr();
        let fb_len = framebuffer.info.byte_len as u64;

        let map_offset = offset * 4096;
        let map_len = pages * 4096;

        if map_offset + map_len > fb_len {
            return Err(ObjectError::InvalidArguments);
        }

        let start_virt = VirtAddr::new(fb_ptr as u64 + map_offset);
        let start_phys = MAPPER
            .get()
            .unwrap()
            .lock()
            .translate_addr(start_virt)
            .ok_or(ObjectError::InvalidArguments)?;

        let start_frame = PhysFrame::containing_address(start_phys);

        let user_addr = with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                permissions,
                Data::Shared { start: start_frame },
            )
        });

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
            ConfigurateRequest::FbTakeControl => *self.used_by_user.lock() = true,
            ConfigurateRequest::FbRelease => *self.used_by_user.lock() = false,
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
