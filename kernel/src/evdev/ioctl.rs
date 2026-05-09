use alloc::{vec, vec::Vec};

use crate::{
    memory::user_safe,
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{device_info::LinuxInputId, object::EventDeviceClientObject};

const EV_VERSION: i32 = 0x01_00_01;

const IOC_NRBITS: u64 = 8;
const IOC_TYPEBITS: u64 = 8;
const IOC_SIZEBITS: u64 = 14;
const IOC_NRMASK: u64 = (1 << IOC_NRBITS) - 1;
const IOC_TYPEMASK: u64 = (1 << IOC_TYPEBITS) - 1;
const IOC_SIZEMASK: u64 = (1 << IOC_SIZEBITS) - 1;
const IOC_NRSHIFT: u64 = 0;
const IOC_TYPESHIFT: u64 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u64 = IOC_TYPESHIFT + IOC_TYPEBITS;

pub(super) fn handle_ioctl(
    client: &EventDeviceClientObject,
    request: u64,
    arg: u64,
) -> ObjectResult<isize> {
    if ioc_type(request) != b'E' {
        return Err(ObjectError::InvalidRequest);
    }

    let nr = ioc_nr(request);
    let size = ioc_size(request);
    let kind = client.kind;

    match nr {
        0x01 => {
            user_safe::write(arg as *mut i32, &EV_VERSION)
                .map_err(|_| ObjectError::InvalidArguments)?;
            Ok(0)
        }
        0x02 => {
            let id = kind.input_id();
            user_safe::write(arg as *mut LinuxInputId, &id)
                .map_err(|_| ObjectError::InvalidArguments)?;
            Ok(0)
        }
        0x03 => {
            let rep = [250u32, 33u32];
            user_safe::write(arg as *mut [u32; 2], &rep)
                .map_err(|_| ObjectError::InvalidArguments)?;
            Ok(0)
        }
        0x06 => write_bytes_ioctl(arg, size, kind.name().as_bytes()),
        0x07 => write_bytes_ioctl(arg, size, kind.phys().as_bytes()),
        0x08 => write_bytes_ioctl(arg, size, &[]),
        0x09 => {
            let props = kind.supports_properties();
            write_fixed_sized_ioctl(arg, size, &props)
        }
        0x18 => {
            let state = client.state.lock();
            write_fixed_sized_ioctl(arg, size, &state.key_state)
        }
        0x19 | 0x1b => write_fixed_sized_ioctl(arg, size, &[]),
        0x20..=0x3f => {
            let bits = kind.supported_event_bits((nr - 0x20) as u8);
            write_fixed_sized_ioctl(arg, size, &bits)
        }
        0x90 => handle_grab_ioctl(client, arg),
        0x91 => handle_revoke_ioctl(client, arg),
        0xa0 => {
            if arg == 0 {
                return Err(ObjectError::InvalidArguments);
            }
            let clock_id = unsafe { *(arg as *const i32) };
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

fn write_bytes_ioctl(arg: u64, size: usize, bytes: &[u8]) -> ObjectResult<isize> {
    let mut data = Vec::with_capacity(bytes.len() + 1);
    data.extend_from_slice(bytes);
    data.push(0);
    write_fixed_sized_ioctl(arg, size, &data)
}

fn write_fixed_sized_ioctl(arg: u64, size: usize, source: &[u8]) -> ObjectResult<isize> {
    if size == 0 {
        return Ok(0);
    }
    if arg == 0 {
        return Err(ObjectError::InvalidArguments);
    }

    let mut out = vec![0u8; size];
    let copy_len = out.len().min(source.len());
    out[..copy_len].copy_from_slice(&source[..copy_len]);
    user_safe::write(arg as *mut u8, &out[..]).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

fn ioc_nr(request: u64) -> u64 {
    (request >> IOC_NRSHIFT) & IOC_NRMASK
}

fn ioc_type(request: u64) -> u8 {
    ((request >> IOC_TYPESHIFT) & IOC_TYPEMASK) as u8
}

fn ioc_size(request: u64) -> usize {
    ((request >> IOC_SIZESHIFT) & IOC_SIZEMASK) as usize
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::{
        EV_VERSION, ioc_nr, ioc_size, ioc_type, write_bytes_ioctl, write_fixed_sized_ioctl,
    };
    use crate::object::error::ObjectError;

    fn ioc_request(ty: u8, nr: u8, size: usize) -> u64 {
        (nr as u64) | ((ty as u64) << 8) | ((size as u64) << 16)
    }

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
        let request = ioc_request(b'E', 0x06, 32);
        assert_eq!(ioc_type(request), b'E');
        assert_eq!(ioc_nr(request), 0x06);
        assert_eq!(ioc_size(request), 32);
        assert_eq!(EV_VERSION, 0x01_00_01);
    }

    fn evdev_ioctl_fixed_sized_writes_copy_truncate_and_reject_null_pointers() {
        let mut out = [0xaau8; 6];
        assert_eq!(
            write_fixed_sized_ioctl(out.as_mut_ptr() as u64, out.len(), &[1, 2, 3]).unwrap(),
            0
        );
        assert_eq!(out, [1, 2, 3, 0, 0, 0]);

        let mut short = [0xaau8; 2];
        assert_eq!(
            write_fixed_sized_ioctl(short.as_mut_ptr() as u64, short.len(), &[9, 8, 7]).unwrap(),
            0
        );
        assert_eq!(short, [9, 8]);

        assert_eq!(write_fixed_sized_ioctl(0, 0, &[1]).unwrap(), 0);
        assert!(matches!(
            write_fixed_sized_ioctl(0, 2, &[1]),
            Err(ObjectError::InvalidArguments)
        ));
    }

    fn evdev_ioctl_string_writes_append_nul_and_truncate_predictably() {
        let mut out = [0xaau8; 8];
        assert_eq!(
            write_bytes_ioctl(out.as_mut_ptr() as u64, out.len(), b"mouse").unwrap(),
            0
        );
        assert_eq!(&out, b"mouse\0\0\0");

        let mut short = [0xaau8; 4];
        assert_eq!(
            write_bytes_ioctl(short.as_mut_ptr() as u64, short.len(), b"keyboard").unwrap(),
            0
        );
        assert_eq!(&short, b"keyb");

        let mut empty = vec![0xaa; 3];
        assert_eq!(
            write_bytes_ioctl(empty.as_mut_ptr() as u64, empty.len(), b"").unwrap(),
            0
        );
        assert_eq!(empty, vec![0, 0xaa, 0xaa]);
    }
}
