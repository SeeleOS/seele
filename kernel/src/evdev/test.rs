use crate::evdev::{device_info::EventDeviceKind, event_bits::KEY_BITMAP_BYTES};

crate::test!("evdev metadata and bitmap helpers", || {
    event_devices_report_stable_names_minors_and_ids();
    event_bitmaps_cover_keyboard_and_mouse_capabilities();
});

fn bit_is_set(bytes: &[u8], bit: usize) -> bool {
    bytes
        .get(bit / 8)
        .is_some_and(|byte| (byte & (1 << (bit % 8))) != 0)
}

fn event_devices_report_stable_names_minors_and_ids() {
    assert_eq!(
        EventDeviceKind::Keyboard.name(),
        "AT Translated Set 2 keyboard"
    );
    assert_eq!(EventDeviceKind::Mouse.name(), "PS/2 Generic Mouse");
    assert_eq!(EventDeviceKind::Keyboard.minor(), 64);
    assert_eq!(EventDeviceKind::Mouse.minor(), 65);
    assert_eq!(EventDeviceKind::Keyboard.input_id().bustype, 0x11);
}

fn event_bitmaps_cover_keyboard_and_mouse_capabilities() {
    let keyboard_events = EventDeviceKind::Keyboard.supported_event_bits(0);
    assert!(bit_is_set(&keyboard_events, 0));
    assert!(bit_is_set(&keyboard_events, 1));
    assert!(!bit_is_set(&keyboard_events, 2));

    let keyboard_keys = EventDeviceKind::Keyboard.supported_event_bits(1);
    assert_eq!(keyboard_keys.len(), KEY_BITMAP_BYTES);
    assert!(bit_is_set(&keyboard_keys, 1));
    assert!(bit_is_set(&keyboard_keys, 127));
    assert!(!bit_is_set(&keyboard_keys, 128));

    let mouse_events = EventDeviceKind::Mouse.supported_event_bits(0);
    assert!(bit_is_set(&mouse_events, 2));
    let mouse_props = EventDeviceKind::Mouse.supports_properties();
    assert!(bit_is_set(&mouse_props, 0));
}
