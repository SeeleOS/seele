use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::memory::utils::Mut;
use lazy_static::lazy_static;
use x86_64::VirtAddr;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::{addrspace::mem_area::Data, protection::Protection},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        misc::ObjectResult,
        open_state::OpenState,
        queue_helpers::read_or_block_with_flags,
        traits::{Configuratable, MemoryMappable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::misc::with_current_process,
    thread::yielding::WakeType,
};

use super::{card::CARD0_RDEV, configure, events, state::DrmState};

lazy_static! {
    pub(super) static ref DRM_STATE: Mut<DrmState> = Mut::new(DrmState::new());
    pub(super) static ref DRM_EVENT_QUEUE: Mut<VecDeque<u8>> = Mut::new(VecDeque::new());
}

pub(super) const DRM_BUFFER_OFFSET_BASE: u64 = 0x1000_0000;

#[derive(Debug, Default)]
pub struct DrmCardObject {
    open_state: OpenState,
}

impl Object for DrmCardObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("mappable", MemoryMappable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("statable", Statable);
}

impl Configuratable for DrmCardObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        configure::handle_configure(request)
    }
}

impl MemoryMappable for DrmCardObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<VirtAddr> {
        let (frames, shared_flags) = DRM_STATE.lock().dumb_buffer_for_mapping(offset, pages)?;

        let mapped = with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                protection,
                Data::Shared {
                    frames,
                    flags: shared_flags,
                },
            )
        });
        Ok(mapped)
    }
}

impl Readable for DrmCardObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, FileFlags::empty())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::IO, events::try_read_drm_events)
    }
}

impl Pollable for DrmCardObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && !DRM_EVENT_QUEUE.lock().is_empty()
    }
}

impl Statable for DrmCardObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o660, CARD0_RDEV)
    }
}
