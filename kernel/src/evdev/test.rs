use core::mem::size_of;

use crate::{
    evdev::{
        KEYBOARD_EVENT_DEVICE, MOUSE_EVENT_DEVICE, device_info::EventDeviceKind,
        event_bits::KEY_BITMAP_BYTES,
    },
    object::{
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{EVDEV_IOCTL_TYPE, ioctl_request},
        traits::Configuratable,
    },
};

crate::test!(
    evdev_metadata,
    "event devices report stable names minors and ids",
    event_devices_report_stable_names_minors_and_ids
);
crate::test!(
    evdev_bitmaps,
    "event bitmaps cover keyboard and mouse capabilities",
    event_bitmaps_cover_keyboard_and_mouse_capabilities
);
crate::test!(
    evdev_ioctl_semantics,
    "evdev ioctls follow linux rules",
    evdev_ioctls_follow_linux_rules
);

fn evdev_request(nr: u8, size: usize) -> u64 {
    ioctl_request(0, EVDEV_IOCTL_TYPE, nr, size)
}

fn evdev_config(nr: u8, size: usize, arg: u64) -> ConfigurateRequest {
    ConfigurateRequest::new(evdev_request(nr, size), arg).unwrap()
}

fn raw_config(request: u64, arg: u64) -> ConfigurateRequest {
    ConfigurateRequest::new(request, arg).unwrap()
}

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

fn evdev_ioctls_follow_linux_rules() {
    let keyboard = KEYBOARD_EVENT_DEVICE.open();

    let mut version = 0i32;
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x01,
                size_of::<i32>(),
                (&mut version as *mut i32) as u64
            ))
            .unwrap(),
        0
    );
    assert_eq!(version, 0x01_00_01);

    let mut id = super::device_info::LinuxInputId {
        bustype: 0,
        vendor: 0,
        product: 0,
        version: 0,
    };
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x02,
                size_of::<super::device_info::LinuxInputId>(),
                (&mut id as *mut super::device_info::LinuxInputId) as u64,
            ))
            .unwrap(),
        0
    );
    assert_eq!(id.bustype, 0x11);
    assert_eq!(id.product, 0x0001);

    let mut rep = [0u32; 2];
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x03,
                size_of::<[u32; 2]>(),
                rep.as_mut_ptr() as u64
            ))
            .unwrap(),
        0
    );
    assert_eq!(rep, [250, 33]);

    let mut name = [0xaa; 32];
    assert_eq!(
        keyboard
            .configure(evdev_config(0x06, name.len(), name.as_mut_ptr() as u64))
            .unwrap(),
        0
    );
    assert_eq!(&name[..28], b"AT Translated Set 2 keyboard");
    assert_eq!(name[28], 0);

    let mut phys = [0xaa; 32];
    assert_eq!(
        keyboard
            .configure(evdev_config(0x07, phys.len(), phys.as_mut_ptr() as u64))
            .unwrap(),
        0
    );
    assert_eq!(&phys[..21], b"isa0060/serio0/input0");
    assert_eq!(phys[21], 0);

    let mut uniq = [0xaa; 4];
    assert_eq!(
        keyboard
            .configure(evdev_config(0x08, uniq.len(), uniq.as_mut_ptr() as u64))
            .unwrap(),
        0
    );
    assert_eq!(uniq[0], 0);

    let mouse = MOUSE_EVENT_DEVICE.open();
    let mut props = [0u8; 4];
    assert_eq!(
        mouse
            .configure(evdev_config(0x09, props.len(), props.as_mut_ptr() as u64))
            .unwrap(),
        0
    );
    assert!(bit_is_set(&props, 0));

    KEYBOARD_EVENT_DEVICE.push_key_event(30, true);
    let mut key_state = [0u8; KEY_BITMAP_BYTES];
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x18,
                key_state.len(),
                key_state.as_mut_ptr() as u64,
            ))
            .unwrap(),
        0
    );
    assert!(bit_is_set(&key_state, 30));

    let mut led = [0xaa; 4];
    let mut snd = [0xaa; 4];
    let mut sw = [0xaa; 4];
    for (nr, out) in [(0x19, &mut led), (0x1a, &mut snd), (0x1b, &mut sw)] {
        assert_eq!(
            keyboard
                .configure(evdev_config(nr, out.len(), out.as_mut_ptr() as u64))
                .unwrap(),
            0
        );
        assert_eq!(out[0], 0);
    }

    let mut event_bits = [0u8; 1];
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x20,
                event_bits.len(),
                event_bits.as_mut_ptr() as u64,
            ))
            .unwrap(),
        0
    );
    assert!(bit_is_set(&event_bits, 0));
    assert!(bit_is_set(&event_bits, 1));

    let mut truncated_key_bits = [0u8; 1];
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0x21,
                truncated_key_bits.len(),
                truncated_key_bits.as_mut_ptr() as u64,
            ))
            .unwrap(),
        0
    );
    assert_eq!(truncated_key_bits, [0xfe]);

    let mut rel_bits = [0u8; 1];
    assert_eq!(
        mouse
            .configure(evdev_config(
                0x22,
                rel_bits.len(),
                rel_bits.as_mut_ptr() as u64,
            ))
            .unwrap(),
        0
    );
    assert!(bit_is_set(&rel_bits, 0));
    assert!(bit_is_set(&rel_bits, 1));

    let grabber = KEYBOARD_EVENT_DEVICE.open();
    let waiter = KEYBOARD_EVENT_DEVICE.open();
    assert_eq!(grabber.configure(evdev_config(0x90, 0, 1)).unwrap(), 0);
    assert!(matches!(
        waiter.configure(evdev_config(0x90, 0, 1)),
        Err(ObjectError::Busy)
    ));
    assert_eq!(grabber.configure(evdev_config(0x90, 0, 0)).unwrap(), 0);
    assert!(matches!(
        grabber.configure(evdev_config(0x90, 0, 2)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut clock_id = 7i32;
    assert_eq!(
        keyboard
            .configure(evdev_config(
                0xa0,
                size_of::<i32>(),
                (&mut clock_id as *mut i32) as u64
            ))
            .unwrap(),
        0
    );
    assert_eq!(keyboard.state.lock().clock_id, 7);
    assert!(matches!(
        keyboard.configure(evdev_config(0xa0, size_of::<i32>(), 0)),
        Err(ObjectError::BadAddress)
    ));

    let revoked = KEYBOARD_EVENT_DEVICE.open();
    assert_eq!(revoked.configure(evdev_config(0x91, 0, 0)).unwrap(), 0);
    assert!(matches!(
        revoked.configure(evdev_config(
            0x01,
            size_of::<i32>(),
            (&mut version as *mut i32) as u64
        )),
        Err(ObjectError::DeviceRevoked)
    ));
    assert!(matches!(
        keyboard.configure(evdev_config(0x91, 0, 1)),
        Err(ObjectError::InvalidArguments)
    ));

    assert!(matches!(
        keyboard.configure(evdev_config(0x06, 4, 0)),
        Err(ObjectError::BadAddress)
    ));
    assert!(matches!(
        keyboard.configure(raw_config(
            ioctl_request(0, b'X', 0x01, size_of::<i32>()),
            (&mut version as *mut i32) as u64,
        )),
        Err(ObjectError::InvalidRequest)
    ));
    assert!(matches!(
        keyboard.configure(raw_config(evdev_request(0xff, 0), 0)),
        Err(ObjectError::InvalidRequest)
    ));
}
