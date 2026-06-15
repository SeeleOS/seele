use alloc::vec::Vec;

use super::socket::{NetlinkMessageHeader, RouteAttributeHeader};

pub(super) fn append_attribute(bytes: &mut Vec<u8>, attr_type: u16, payload: &[u8]) {
    let attr_len = core::mem::size_of::<RouteAttributeHeader>() + payload.len();
    let header = RouteAttributeHeader {
        rta_len: attr_len as u16,
        rta_type: attr_type,
    };
    append_struct(bytes, &header);
    bytes.extend_from_slice(payload);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

pub(super) fn append_string_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: &str) {
    let mut payload = value.as_bytes().to_vec();
    payload.push(0);
    append_attribute(bytes, attr_type, &payload);
}

pub(super) fn append_u8_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: u8) {
    append_attribute(bytes, attr_type, &[value]);
}

pub(super) fn append_u32_attribute(bytes: &mut Vec<u8>, attr_type: u16, value: u32) {
    append_attribute(bytes, attr_type, &value.to_ne_bytes());
}

pub(super) fn append_struct<T>(bytes: &mut Vec<u8>, value: &T) {
    bytes.extend_from_slice(unsafe {
        core::slice::from_raw_parts((value as *const T).cast::<u8>(), core::mem::size_of::<T>())
    });
}

pub(super) fn finalize_message_length(bytes: &mut [u8]) {
    let header = NetlinkMessageHeader {
        nlmsg_len: bytes.len() as u32,
        nlmsg_type: u16::from_ne_bytes([bytes[4], bytes[5]]),
        nlmsg_flags: u16::from_ne_bytes([bytes[6], bytes[7]]),
        nlmsg_seq: u32::from_ne_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        nlmsg_pid: u32::from_ne_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
    };
    bytes[..core::mem::size_of::<NetlinkMessageHeader>()].copy_from_slice(unsafe {
        core::slice::from_raw_parts(
            (&header as *const NetlinkMessageHeader).cast::<u8>(),
            core::mem::size_of::<NetlinkMessageHeader>(),
        )
    });
}

pub(super) fn find_attribute(payload: &[u8], mut offset: usize, attr_type: u16) -> Option<&[u8]> {
    while offset + core::mem::size_of::<RouteAttributeHeader>() <= payload.len() {
        let header = unsafe {
            core::ptr::read_unaligned(payload[offset..].as_ptr().cast::<RouteAttributeHeader>())
        };
        let attr_len = usize::from(header.rta_len);
        if attr_len < core::mem::size_of::<RouteAttributeHeader>() {
            return None;
        }
        let attr_end = offset.checked_add(attr_len)?;
        if attr_end > payload.len() {
            return None;
        }
        if header.rta_type == attr_type {
            return Some(&payload[offset + core::mem::size_of::<RouteAttributeHeader>()..attr_end]);
        }
        offset = align_to_4(attr_end);
    }
    None
}

pub(super) fn parse_netlink_string(bytes: &[u8]) -> Option<&str> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    core::str::from_utf8(bytes).ok()
}

pub(super) fn parse_i32_attribute(bytes: &[u8]) -> Option<i32> {
    Some(i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?))
}

pub(super) fn read_struct_prefix<T: Copy>(bytes: &[u8]) -> Option<T> {
    if bytes.len() < core::mem::size_of::<T>() {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

pub(super) fn align_to_4(value: usize) -> usize {
    (value + 3) & !3
}
