use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    filesystem::{info::DirectoryContentInfo, vfs_traits::DirectoryContentType},
    net,
};

pub(super) const PROC_NET_INODE: u64 = 0x3017;
pub(super) const PROC_NET_DEV_INODE: u64 = 0x3018;
pub(super) const PROC_NET_ROUTE_INODE: u64 = 0x3019;
pub(super) const PROC_NET_IF_INET6_INODE: u64 = 0x301a;

pub(super) fn proc_net_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("dev".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("route".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("if_inet6".into(), DirectoryContentType::File),
    ]
}

pub(super) fn proc_net_dev_bytes() -> Vec<u8> {
    let mut out = String::from(
        "Inter-|   Receive                                                |  Transmit\n",
    );
    out.push_str(
        " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
    );

    for interface in net::interfaces() {
        out.push_str(&format!(
            "{:>6}: {:>7} {:>7} {:>4} {:>4} {:>4} {:>5} {:>10} {:>9} {:>7} {:>7} {:>4} {:>4} {:>4} {:>5} {:>7} {:>10}\n",
            interface.name,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ));
    }

    out.into_bytes()
}

pub(super) fn proc_net_route_bytes() -> Vec<u8> {
    let mut out = String::from(
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n",
    );

    for interface in net::interfaces() {
        if let Some((addr, prefix_len)) = interface.ipv4 {
            let mask = prefix_mask(prefix_len);
            let destination = if interface.loopback {
                addr
            } else {
                ipv4_and(addr, mask)
            };
            let gateway = interface.gateway.unwrap_or([0, 0, 0, 0]);
            let flags = if interface.gateway.is_some() {
                0x0003
            } else {
                0x0001
            };
            out.push_str(&format!(
                "{}\t{:08X}\t{:08X}\t{:04X}\t0\t0\t0\t{:08X}\t0\t0\t0\n",
                interface.name,
                proc_hex_ipv4(destination),
                proc_hex_ipv4(gateway),
                flags,
                proc_hex_ipv4(mask),
            ));
        }
    }

    out.into_bytes()
}

pub(super) fn proc_net_if_inet6_bytes() -> Vec<u8> {
    String::from("00000000000000000000000000000001 01 80 10 80       lo\n").into_bytes()
}

fn prefix_mask(prefix_len: u8) -> [u8; 4] {
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix_len))
    };
    mask.to_be_bytes()
}

fn ipv4_and(addr: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    [
        addr[0] & mask[0],
        addr[1] & mask[1],
        addr[2] & mask[2],
        addr[3] & mask[3],
    ]
}

fn proc_hex_ipv4(addr: [u8; 4]) -> u32 {
    u32::from_le_bytes(addr)
}
