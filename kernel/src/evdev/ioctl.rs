use alloc::{vec, vec::Vec};

use crate::{
    memory::user_safe,
    object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult},
};

use super::{device_info::LinuxInputId, object::EventDeviceClientObject};

const EV_VERSION: i32 = 0x01_00_01;

pub(super) fn is_evdev_ioctl(request: &ConfigurateRequest) -> bool {
    matches!(
        request,
        ConfigurateRequest::EvdevGetVersion(_)
            | ConfigurateRequest::EvdevGetId(_)
            | ConfigurateRequest::EvdevGetRepeat(_)
            | ConfigurateRequest::EvdevGetName { .. }
            | ConfigurateRequest::EvdevGetPhys { .. }
            | ConfigurateRequest::EvdevGetUniq { .. }
            | ConfigurateRequest::EvdevGetProp { .. }
            | ConfigurateRequest::EvdevGetKey { .. }
            | ConfigurateRequest::EvdevGetLed { .. }
            | ConfigurateRequest::EvdevGetSnd { .. }
            | ConfigurateRequest::EvdevGetSw { .. }
            | ConfigurateRequest::EvdevGetBit { .. }
            | ConfigurateRequest::EvdevGrab(_)
            | ConfigurateRequest::EvdevRevoke(_)
            | ConfigurateRequest::EvdevSetClockId(_)
    )
}

pub(super) fn handle_ioctl(
    client: &EventDeviceClientObject,
    request: ConfigurateRequest,
) -> ObjectResult<isize> {
    let kind = client.kind;

    match request {
        ConfigurateRequest::EvdevGetVersion(ptr) => {
            user_safe::write(ptr, &EV_VERSION).map_err(|_| ObjectError::BadAddress)?;
            Ok(0)
        }
        ConfigurateRequest::EvdevGetId(ptr) => {
            let id = kind.input_id();
            user_safe::write(ptr as *mut LinuxInputId, &id).map_err(|_| ObjectError::BadAddress)?;
            Ok(0)
        }
        ConfigurateRequest::EvdevGetRepeat(ptr) => {
            let rep = [250u32, 33u32];
            user_safe::write(ptr, &rep).map_err(|_| ObjectError::BadAddress)?;
            Ok(0)
        }
        ConfigurateRequest::EvdevGetName { ptr, len } => {
            write_bytes_ioctl(ptr, len, kind.name().as_bytes())
        }
        ConfigurateRequest::EvdevGetPhys { ptr, len } => {
            write_bytes_ioctl(ptr, len, kind.phys().as_bytes())
        }
        ConfigurateRequest::EvdevGetUniq { ptr, len } => write_bytes_ioctl(ptr, len, &[]),
        ConfigurateRequest::EvdevGetProp { ptr, len } => {
            let props = kind.supports_properties();
            write_fixed_sized_ioctl(ptr, len, &props)
        }
        ConfigurateRequest::EvdevGetKey { ptr, len } => {
            let state = client.state.lock();
            write_fixed_sized_ioctl(ptr, len, &state.key_state)
        }
        ConfigurateRequest::EvdevGetLed { ptr, len }
        | ConfigurateRequest::EvdevGetSnd { ptr, len }
        | ConfigurateRequest::EvdevGetSw { ptr, len } => write_fixed_sized_ioctl(ptr, len, &[]),
        ConfigurateRequest::EvdevGetBit {
            event_type,
            ptr,
            len,
        } => {
            let bits = kind.supported_event_bits(event_type);
            write_fixed_sized_ioctl(ptr, len, &bits)
        }
        ConfigurateRequest::EvdevGrab(value) => handle_grab_ioctl(client, value),
        ConfigurateRequest::EvdevRevoke(value) => handle_revoke_ioctl(client, value),
        ConfigurateRequest::EvdevSetClockId(ptr) => {
            let clock_id = user_safe::read(ptr).map_err(|_| ObjectError::BadAddress)?;
            client.state.lock().clock_id = clock_id;
            Ok(0)
        }
        _ => Err(ObjectError::InvalidRequest),
    }
}

fn handle_grab_ioctl(client: &EventDeviceClientObject, arg: u64) -> ObjectResult<isize> {
    let Some(hub) = client.hub.upgrade() else {
        return Err(ObjectError::DoesNotExist);
    };

    match arg {
        0 => {
            hub.ungrab(client.client_id);
            Ok(0)
        }
        1 => {
            if hub.grab(client.client_id) {
                Ok(0)
            } else {
                Err(ObjectError::Busy)
            }
        }
        _ => Err(ObjectError::InvalidArguments),
    }
}

fn handle_revoke_ioctl(client: &EventDeviceClientObject, arg: u64) -> ObjectResult<isize> {
    if arg != 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let client = client
        .hub
        .upgrade()
        .and_then(|hub| {
            hub.clients.lock().iter().find_map(|weak| {
                weak.upgrade()
                    .filter(|candidate| candidate.client_id == client.client_id)
            })
        })
        .ok_or(ObjectError::DoesNotExist)?;
    client.revoke();
    Ok(0)
}

fn write_bytes_ioctl(ptr: *mut u8, size: usize, bytes: &[u8]) -> ObjectResult<isize> {
    let mut data = Vec::with_capacity(bytes.len() + 1);
    data.extend_from_slice(bytes);
    data.push(0);
    write_fixed_sized_ioctl(ptr, size, &data)
}

fn write_fixed_sized_ioctl(ptr: *mut u8, size: usize, source: &[u8]) -> ObjectResult<isize> {
    if size == 0 {
        return Ok(0);
    }

    let mut out = vec![0u8; size];
    let copy_len = out.len().min(source.len());
    out[..copy_len].copy_from_slice(&source[..copy_len]);
    user_safe::write(ptr, &out[..]).map_err(|_| ObjectError::BadAddress)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{EV_VERSION, write_bytes_ioctl, write_fixed_sized_ioctl};
    use crate::object::error::ObjectError;
    use crate::object::linux_ioctl::{ioctl_nr, ioctl_request, ioctl_size, ioctl_type};

    crate::test!(
        evdev_ioctl_request_decode,
        "evdev ioctl helpers decode ioc fields",
        evdev_ioctl_helpers_decode_ioc_fields
    );
    crate::test!(
        evdev_ioctl_fixed_size_copy,
        "evdev ioctl fixed-sized writes copy truncate and reject null pointers",
        evdev_ioctl_fixed_sized_writes_copy_truncate_and_reject_null_pointers
    );
    crate::test!(
        evdev_ioctl_string_copy,
        "evdev ioctl string writes append nul and truncate predictably",
        evdev_ioctl_string_writes_append_nul_and_truncate_predictably
    );

    fn evdev_ioctl_helpers_decode_ioc_fields() {
        let request = ioctl_request(0, b'E', 0x06, 32);
        assert_eq!(ioctl_type(request), b'E');
        assert_eq!(ioctl_nr(request), 0x06);
        assert_eq!(ioctl_size(request), 32);
        assert_eq!(EV_VERSION, 0x01_00_01);
    }

    fn evdev_ioctl_fixed_sized_writes_copy_truncate_and_reject_null_pointers() {
        let mut out = [0xaau8; 6];
        assert_eq!(
            write_fixed_sized_ioctl(out.as_mut_ptr(), out.len(), &[1, 2, 3]).unwrap(),
            0
        );
        assert_eq!(out, [1, 2, 3, 0, 0, 0]);

        let mut short = [0xaau8; 2];
        assert_eq!(
            write_fixed_sized_ioctl(short.as_mut_ptr(), short.len(), &[9, 8, 7]).unwrap(),
            0
        );
        assert_eq!(short, [9, 8]);

        assert_eq!(
            write_fixed_sized_ioctl(core::ptr::null_mut(), 0, &[1]).unwrap(),
            0
        );
        assert!(matches!(
            write_fixed_sized_ioctl(core::ptr::null_mut(), 2, &[1]),
            Err(ObjectError::BadAddress)
        ));
    }

    fn evdev_ioctl_string_writes_append_nul_and_truncate_predictably() {
        let mut out = [0xaau8; 8];
        assert_eq!(
            write_bytes_ioctl(out.as_mut_ptr(), out.len(), b"mouse").unwrap(),
            0
        );
        assert_eq!(&out, b"mouse\0\0\0");

        let mut short = [0xaau8; 4];
        assert_eq!(
            write_bytes_ioctl(short.as_mut_ptr(), short.len(), b"keyboard").unwrap(),
            0
        );
        assert_eq!(&short, b"keyb");

        let mut empty = vec![0xaa; 3];
        assert_eq!(
            write_bytes_ioctl(empty.as_mut_ptr(), empty.len(), b"").unwrap(),
            0
        );
        assert_eq!(empty, vec![0, 0, 0]);
    }
}
