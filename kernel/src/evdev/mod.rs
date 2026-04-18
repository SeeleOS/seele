mod device_info;
mod event_bits;
mod ioctl;
mod object;
mod ps2;
mod queue;

pub use object::{EventDeviceObject, KEYBOARD_EVENT_DEVICE, MOUSE_EVENT_DEVICE};
pub use ps2::{init_mouse_packet_decoder, process_ps2_mouse_packet, push_keyboard_event};
