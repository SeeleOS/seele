use alloc::{vec, vec::Vec};
use spin::Mutex;

use crate::{
    memory::user_safe,
    object::{error::ObjectError, misc::ObjectResult},
};

use super::{
    device_info::{EventDeviceKind, LinuxInputId},
    queue::EventDeviceState,
};

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
    kind: EventDeviceKind,
    state: &Mutex<EventDeviceState>,
    request: u64,
    arg: u64,
) -> ObjectResult<isize> {
    if ioc_type(request) != b'E' {
        return Err(ObjectError::InvalidRequest);
    }

    let nr = ioc_nr(request);
    let size = ioc_size(request);

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
            let state = state.lock();
            write_fixed_sized_ioctl(arg, size, &state.key_state)
        }
        0x19 | 0x1b => write_fixed_sized_ioctl(arg, size, &[]),
        0x20..=0x3f => {
            let bits = kind.supported_event_bits((nr - 0x20) as u8);
            write_fixed_sized_ioctl(arg, size, &bits)
        }
        0x90 | 0x91 => Ok(0),
        0xa0 => {
            if arg == 0 {
                return Err(ObjectError::InvalidArguments);
            }
            let clock_id = unsafe { *(arg as *const i32) };
            state.lock().clock_id = clock_id;
            Ok(0)
        }
        _ => Err(ObjectError::InvalidRequest),
    }
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
