mod accept;
mod bind;
mod connect;
mod create;
mod datagram;
mod drop_impl;
mod error;
mod inet;
mod name;
mod object;
mod packet;
mod pair;
mod registry;
mod self_ref;
mod socket_like;
mod sockopt;
mod state;
mod stream;
mod traits_object;
mod traits_poll;
mod traits_read;
mod traits_stat;
mod traits_write;
mod wait;
mod wake;

use crate::process::manager::get_current_process;

pub const AF_UNIX: u64 = 1;
pub const AF_INET: u64 = 2;
pub const AF_INET6: u64 = 10;
pub const AF_PACKET: u64 = 17;
pub const AF_NETLINK: u64 = 16;
pub const SOL_IP: u64 = 0;
pub const SOL_TCP: u64 = 6;
pub const SOL_UDP: u64 = 17;
pub const SOL_SOCKET: u64 = 1;
pub const SOL_NETLINK: u64 = 270;
pub const SOCK_STREAM: u64 = 1;
pub const SOCK_DGRAM: u64 = 2;
pub const SOCK_RAW: u64 = 3;
pub const SOCK_SEQPACKET: u64 = 5;
pub const SOCK_NONBLOCK: u64 = 0o4_000;
pub const SOCK_CLOEXEC: u64 = 0o2_000_000;
pub const SO_REUSEADDR: u64 = 2;
pub const SO_TYPE: u64 = 3;
pub const SO_ERROR: u64 = 4;
pub const SO_PRIORITY: u64 = 12;
pub const SO_SNDBUF: u64 = 7;
pub const SO_RCVBUF: u64 = 8;
pub const SO_PASSCRED: u64 = 16;
pub const SO_RCVTIMEO_OLD: u64 = 20;
pub const SO_SNDTIMEO_OLD: u64 = 21;
pub const SO_PASSSEC: u64 = 34;
pub const SO_TIMESTAMP_OLD: u64 = 29;
pub const SO_TIMESTAMPNS_OLD: u64 = 35;
pub const SO_TIMESTAMP_NEW: u64 = 63;
pub const SO_TIMESTAMPNS_NEW: u64 = 64;
pub const SO_RCVTIMEO_NEW: u64 = 66;
pub const SO_SNDTIMEO_NEW: u64 = 67;
pub const SO_ATTACH_FILTER: u64 = 26;
pub const SO_DETACH_FILTER: u64 = 27;
pub const SO_ATTACH_BPF: u64 = 50;
pub const SO_DETACH_BPF: u64 = 68;
pub const SO_PEERCRED: u64 = 17;
pub const SO_ACCEPTCONN: u64 = 30;
pub const SO_PEERSEC: u64 = 31;
pub const SO_SNDBUFFORCE: u64 = 32;
pub const SO_RCVBUFFORCE: u64 = 33;
pub const SO_PROTOCOL: u64 = 38;
pub const SO_DOMAIN: u64 = 39;
pub const SO_PEERGROUPS: u64 = 59;
pub const SO_PASSPIDFD: u64 = 76;
pub const SO_PEERPIDFD: u64 = 77;
pub const SO_PASSRIGHTS: u64 = 83;
pub const NETLINK_ROUTE: u64 = 0;
pub const NETLINK_AUDIT: u64 = 9;
pub const NETLINK_KOBJECT_UEVENT: u64 = 15;
pub const NETLINK_ADD_MEMBERSHIP: u64 = 1;
pub const NETLINK_DROP_MEMBERSHIP: u64 = 2;
pub const NETLINK_PKTINFO: u64 = 3;
pub const NETLINK_LIST_MEMBERSHIPS: u64 = 9;
pub const NETLINK_EXT_ACK: u64 = 11;
pub const NETLINK_GET_STRICT_CHK: u64 = 12;
pub const IPPROTO_TCP: u64 = 6;
pub const IPPROTO_UDP: u64 = 17;
pub const IPPROTO_SCTP: u64 = 132;
pub const IPPROTO_UDPLITE: u64 = 136;
pub const TCP_NODELAY: u64 = 1;

pub use datagram::{DATAGRAM_RECV_CAPACITY, UnixDatagramInner, UnixDatagramMessage};
pub use error::{SocketError, SocketResult};
pub use inet::{InetSocketKind, InetSocketObject};
pub(crate) use name::parse_unix_socket_path;
pub use object::{UnixSocketKind, UnixSocketObject};
pub use packet::PacketSocketObject;
pub(crate) use registry::{UNIX_SOCKET_REGISTRY, UnixSocketRegistryEntry, UnixSocketRegistryKey};
pub use socket_like::SocketLike;
pub use state::{UnixListenerInner, UnixSocketState};
pub use stream::{PendingRights, STREAM_RECV_CAPACITY, SocketPeerCred, UnixStreamInner};
pub(crate) use wake::{wake_io, wake_pollers};

pub(crate) fn current_socket_peer_cred() -> SocketPeerCred {
    let process = get_current_process();
    let process = process.lock();
    SocketPeerCred {
        pid: process.pid.0,
        uid: process.effective_uid,
        gid: process.effective_gid,
    }
}

pub(crate) fn socket_timeout_option_len(option_name: u64) -> Option<usize> {
    match option_name {
        SO_RCVTIMEO_OLD | SO_SNDTIMEO_OLD | SO_RCVTIMEO_NEW | SO_SNDTIMEO_NEW => Some(16),
        _ => None,
    }
}

pub(crate) fn can_set_socket_priority(priority: i32) -> SocketResult<()> {
    if priority < 0 {
        return Err(SocketError::PermissionDenied);
    }
    if priority <= 6 {
        return Ok(());
    }

    const CAP_NET_ADMIN: usize = 12;
    let process = get_current_process();
    let process = process.lock();
    let slot = CAP_NET_ADMIN / 32;
    let mask = 1u32 << (CAP_NET_ADMIN % 32);
    if (process.capability_effective[slot] & mask) != 0 {
        Ok(())
    } else {
        Err(SocketError::PermissionDenied)
    }
}

#[cfg(test)]
mod test;
