use core::{mem::size_of, slice};

use crate::{
    drm::client::{DRM_EVENT_FLIP_COMPLETE, DrmEvent, DrmEventVblank, DrmWaitVblankReply},
    misc::time::Time,
    object::{device::DEVICES, queue_helpers::copy_from_queue},
    polling::event::PollableEvent,
    thread::THREAD_MANAGER,
};

use super::object::{DRM_EVENT_QUEUE, DRM_STATE};

pub(super) fn queue_page_flip_event(user_data: u64, crtc_id: u32) {
    let sequence = next_vblank_sequence();
    queue_vblank_event(user_data, crtc_id, DRM_EVENT_FLIP_COMPLETE, sequence);
}

pub(super) fn make_vblank_reply(type_: u32, sequence: u32) -> DrmWaitVblankReply {
    let now = Time::since_boot();
    DrmWaitVblankReply {
        type_,
        sequence,
        tv_sec: now.as_seconds().min(i64::MAX as u64) as i64,
        tv_usec: now.subsec_microseconds() as i64,
    }
}

pub(super) fn queue_vblank_event(user_data: u64, crtc_id: u32, event_type: u32, sequence: u32) {
    let reply = make_vblank_reply(0, sequence);
    let event = DrmEventVblank {
        base: DrmEvent {
            type_: event_type,
            length: size_of::<DrmEventVblank>() as u32,
        },
        user_data,
        tv_sec: reply.tv_sec.clamp(0, i64::from(u32::MAX)) as u32,
        tv_usec: reply.tv_usec.clamp(0, i64::from(u32::MAX)) as u32,
        sequence: reply.sequence,
        crtc_id,
    };
    let bytes = unsafe {
        slice::from_raw_parts(
            (&event as *const DrmEventVblank).cast::<u8>(),
            size_of::<DrmEventVblank>(),
        )
    };
    DRM_EVENT_QUEUE.lock().extend(bytes.iter().copied());
    wake_drm_readable();
}

fn next_vblank_sequence() -> u32 {
    let mut state = DRM_STATE.lock();
    let sequence = state.next_flip_sequence;
    state.next_flip_sequence = state.next_flip_sequence.wrapping_add(1);
    sequence
}

fn wake_drm_readable() {
    let drm_object = DEVICES.get("drm-card0").cloned().unwrap();
    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    manager.wake_io();
    manager.wake_poller(drm_object, PollableEvent::CanBeRead);
}

pub(super) fn try_read_drm_events(buffer: &mut [u8]) -> Option<usize> {
    let event_size = size_of::<DrmEventVblank>();
    if buffer.len() < event_size {
        return Some(0);
    }

    let mut queue = DRM_EVENT_QUEUE.lock();
    if queue.len() < event_size {
        return None;
    }

    let events_to_copy = core::cmp::min(buffer.len() / event_size, queue.len() / event_size);
    let bytes_to_copy = events_to_copy * event_size;
    Some(copy_from_queue(&mut queue, &mut buffer[..bytes_to_copy]))
}
