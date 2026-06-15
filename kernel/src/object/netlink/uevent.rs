use alloc::{format, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::socket::NETLINK_KOBJECT_UEVENT;

use super::socket::{NETLINK_SOCKETS, NetlinkSocketAddress};

static NEXT_UEVENT_SEQNUM: AtomicU64 = AtomicU64::new(1);

pub fn broadcast_kobject_uevent(action: &str, devpath: &str, extra_env: &[u8]) {
    let seqnum = NEXT_UEVENT_SEQNUM.fetch_add(1, Ordering::Relaxed);
    let mut message =
        format!("{action}@{devpath}\0ACTION={action}\0DEVPATH={devpath}\0").into_bytes();
    for line in extra_env
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.starts_with(b"ACTION=")
            || line.starts_with(b"DEVPATH=")
            || line.starts_with(b"SEQNUM=")
        {
            continue;
        }
        message.extend_from_slice(line);
        message.push(0);
    }
    message.extend_from_slice(format!("SEQNUM={seqnum}\0").as_bytes());

    let mut sockets = NETLINK_SOCKETS.lock();
    let mut delivered_sockets = Vec::new();
    sockets.retain(|socket| {
        let Some(socket) = socket.upgrade() else {
            return false;
        };
        if socket.protocol() == NETLINK_KOBJECT_UEVENT && socket.receives_group(1) {
            socket.queue_message_with_source(
                message.clone(),
                NetlinkSocketAddress { pid: 0, groups: 1 },
                0,
                0,
            );
            delivered_sockets.push(socket);
        }
        true
    });
    drop(sockets);
    for socket in delivered_sockets {
        socket.wake_read_waiters();
    }
}
