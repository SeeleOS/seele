mod device_info;
mod event_bits;
mod ioctl;
mod object;
mod ps2;
mod queue;

pub use object::{EventDeviceObject, KEYBOARD_EVENT_DEVICE, MOUSE_EVENT_DEVICE, open_event_device};
pub use ps2::{
    init_mouse_packet_decoder, process_ps2_mouse_packet, process_ps2_mouse_packet_deferred_wake,
    push_keyboard_event,
};
