use crate::{
    define_syscall,
    memory::user_safe,
    misc::systemd_perf::{self, PerfBucket},
    net::InetAddress,
    object::netlink::{NetlinkSocketAddress, NetlinkSocketObject},
    object::{
        FileFlags,
        error::ObjectError,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{FdFlags, manager::get_current_process},
    socket::{
        AF_INET, AF_NETLINK, AF_UNIX, InetSocketObject, SOCK_CLOEXEC, SOCK_NONBLOCK, SOL_SOCKET,
        UnixSocketKind, UnixSocketObject, UnixSocketState,
    },
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{string::String, vec, vec::Vec};
use core::{mem, slice};

#[repr(C)]
struct LinuxSockAddrUn {
    sun_family: u16,
    sun_path: [u8; 108],
}

#[repr(C)]
struct LinuxSockAddrNl {
    nl_family: u16,
    nl_pad: u16,
    nl_pid: u32,
    nl_groups: u32,
}

#[repr(C)]
struct LinuxSockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxCmsgHdr {
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxUcred {
    pid: i32,
    uid: u32,
    gid: u32,
}

fn cmsg_align(len: usize) -> usize {
    let align = mem::size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

fn encode_control_message(cmsg_type: i32, payload: &[u8]) -> Vec<u8> {
    let header_space = cmsg_align(mem::size_of::<LinuxCmsgHdr>());
    let control_len = header_space + cmsg_align(payload.len());
    let header = LinuxCmsgHdr {
        cmsg_len: header_space + payload.len(),
        cmsg_level: SOL_SOCKET as i32,
        cmsg_type,
    };
    let mut control = vec![0u8; control_len];
    let header_bytes = unsafe {
        slice::from_raw_parts(
            (&header as *const LinuxCmsgHdr).cast::<u8>(),
            mem::size_of::<LinuxCmsgHdr>(),
        )
    };
    control[..header_bytes.len()].copy_from_slice(header_bytes);
    control[header_space..header_space + payload.len()].copy_from_slice(payload);
    control
}

fn stream_rights_control_bytes_for_read(
    socket: &UnixSocketObject,
    bytes_read: usize,
    peek: bool,
) -> Result<Vec<u8>, SyscallError> {
    let UnixSocketState::Stream(stream) = &*socket.state.lock() else {
        return Ok(Vec::new());
    };

    let ready_rights = if peek {
        stream.peek_ready_rights(bytes_read)
    } else {
        stream.take_ready_rights(bytes_read)
    };
    if ready_rights.is_empty() {
        return Ok(Vec::new());
    }

    let current_process = get_current_process();
    let mut current = current_process.lock();
    let mut control = Vec::new();
    for rights in ready_rights {
        let mut payload = Vec::with_capacity(rights.len() * mem::size_of::<i32>());
        for right in rights {
            let fd = i32::try_from(current.push_object(right))
                .map_err(|_| SyscallError::TooManyOpenFilesProcess)?;
            payload.extend_from_slice(&fd.to_ne_bytes());
        }
        control.extend_from_slice(&encode_control_message(SCM_RIGHTS, &payload));
    }
    Ok(control)
}

fn unix_socket_control_bytes(
    socket: &UnixSocketObject,
    bytes_read: usize,
    peek: bool,
) -> Result<Vec<u8>, SyscallError> {
    let mut control = stream_rights_control_bytes_for_read(socket, bytes_read, peek)?;
    if !*socket.pass_cred.lock() {
        return Ok(control);
    }

    let peer_cred = match &*socket.state.lock() {
        UnixSocketState::Datagram(datagram) => *datagram.peer_cred.lock(),
        UnixSocketState::Stream(stream) => *stream.peer_cred.lock(),
        _ => return Ok(control),
    };
    let credential = LinuxUcred {
        pid: i32::try_from(peer_cred.pid).map_err(|_| SyscallError::InvalidArguments)?,
        uid: peer_cred.uid,
        gid: peer_cred.gid,
    };
    let cred_bytes = unsafe {
        slice::from_raw_parts(
            (&credential as *const LinuxUcred).cast::<u8>(),
            mem::size_of::<LinuxUcred>(),
        )
    };
    control.extend_from_slice(&encode_control_message(SCM_CREDENTIALS, cred_bytes));
    Ok(control)
}

fn netlink_socket_control_bytes(
    socket: &NetlinkSocketObject,
    source_pid: u32,
    uid: u32,
    gid: u32,
) -> Result<Vec<u8>, SyscallError> {
    if !socket.pass_cred_enabled() {
        return Ok(Vec::new());
    }

    let credential = LinuxUcred {
        pid: i32::try_from(source_pid).map_err(|_| SyscallError::InvalidArguments)?,
        uid,
        gid,
    };
    let cred_bytes = unsafe {
        slice::from_raw_parts(
            (&credential as *const LinuxUcred).cast::<u8>(),
            mem::size_of::<LinuxUcred>(),
        )
    };
    Ok(encode_control_message(SCM_CREDENTIALS, cred_bytes))
}

const MSG_PEEK: u64 = 0x2;
const MSG_CTRUNC: i32 = 0x8;
const MSG_DONTWAIT: u64 = 0x40;
const MSG_TRUNC: u64 = 0x20;
const SCM_RIGHTS: i32 = 1;
const SCM_CREDENTIALS: i32 = 2;

enum SocketAddress {
    Inet(InetAddress),
    Unix(String),
    Netlink(NetlinkSocketAddress),
}

fn socket_address_from_raw(
    address: *const u8,
    address_len: u32,
) -> Result<SocketAddress, SyscallError> {
    if address.is_null() || address_len < 2 {
        return Err(SyscallError::BadAddress);
    }
    let family = unsafe { *(address as *const u16) };
    if family == AF_UNIX as u16 {
        let addr = unsafe { &*(address as *const LinuxSockAddrUn) };
        let path_len = (address_len as usize)
            .saturating_sub(2)
            .min(addr.sun_path.len());
        if path_len == 0 {
            return Err(SyscallError::InvalidArguments);
        }

        if addr.sun_path[0] == 0 {
            if path_len <= 1 {
                return Err(SyscallError::InvalidArguments);
            }
            return Ok(SocketAddress::Unix(
                String::from_utf8_lossy(&addr.sun_path[..path_len]).into_owned(),
            ));
        }

        let len = addr.sun_path[..path_len]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_len);
        if len == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        return Ok(SocketAddress::Unix(
            String::from_utf8_lossy(&addr.sun_path[..len]).into_owned(),
        ));
    }

    if family == AF_INET as u16 {
        if (address_len as usize) < mem::size_of::<LinuxSockAddrIn>() {
            return Err(SyscallError::InvalidArguments);
        }
        let addr = unsafe { &*(address as *const LinuxSockAddrIn) };
        return Ok(SocketAddress::Inet(InetAddress::new(
            addr.sin_addr,
            u16::from_be(addr.sin_port),
        )));
    }

    if family == AF_NETLINK as u16 {
        if (address_len as usize) < core::mem::size_of::<LinuxSockAddrNl>() {
            return Err(SyscallError::InvalidArguments);
        }
        let addr = unsafe { &*(address as *const LinuxSockAddrNl) };
        return Ok(SocketAddress::Netlink(NetlinkSocketAddress {
            pid: addr.nl_pid,
            groups: addr.nl_groups,
        }));
    }

    Err(SyscallError::AddressFamilyNotSupported)
}

fn write_socket_name(
    address: *mut u8,
    address_len_ptr: *mut u32,
    name: &[u8],
) -> Result<(), SyscallError> {
    if address_len_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let requested_len = unsafe { *address_len_ptr as usize };
    let copy_len = requested_len.min(name.len());
    if copy_len > 0 && address.is_null() {
        return Err(SyscallError::BadAddress);
    }

    if copy_len > 0 {
        user_safe::write(address, &name[..copy_len])?;
    }
    user_safe::write(address_len_ptr, &(name.len() as u32))?;
    Ok(())
}

fn socket_address_bytes(address: *const u8, address_len: u32) -> Result<Vec<u8>, SyscallError> {
    if address.is_null() || address_len < 2 {
        return Err(SyscallError::BadAddress);
    }
    Ok(unsafe { slice::from_raw_parts(address, address_len as usize) }.to_vec())
}

fn accept_socket(socket: ObjectRef, flags: u32) -> Result<usize, SyscallError> {
    if let Ok(socket) = socket.clone().as_unix_socket() {
        let fd = socket.accept().map_err(ObjectError::from)?;
        let accepted = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let mut file_flags = FileFlags::empty();
        if (flags & SOCK_NONBLOCK as u32) != 0 {
            file_flags.insert(FileFlags::NONBLOCK);
        }
        let _ = accepted.set_flags(file_flags);
        if (flags & SOCK_CLOEXEC as u32) != 0 {
            get_current_process()
                .lock()
                .set_fd_flags(fd, FdFlags::CLOEXEC)
                .map_err(SyscallError::from)?;
        }
        return Ok(fd);
    }

    let accepted: ObjectRef = if let Ok(socket) = socket.as_inet_socket() {
        socket.accept().map_err(ObjectError::from)?
    } else {
        return Err(SyscallError::BadFileDescriptor);
    };

    let mut file_flags = FileFlags::empty();
    if (flags & SOCK_NONBLOCK as u32) != 0 {
        file_flags.insert(FileFlags::NONBLOCK);
    }
    let _ = accepted.clone().set_flags(file_flags);

    let fd_flags = if (flags & SOCK_CLOEXEC as u32) != 0 {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(accepted, fd_flags);
    Ok(fd)
}

define_syscall!(Socket, |domain: u64, kind: u64, protocol: u64| {
    let socket: ObjectRef = if domain == AF_NETLINK {
        NetlinkSocketObject::create(kind, protocol).map_err(ObjectError::from)?
    } else if domain == AF_INET {
        InetSocketObject::create(domain, kind, protocol).map_err(ObjectError::from)?
    } else {
        UnixSocketObject::create(domain, kind, protocol).map_err(ObjectError::from)?
    };
    if (kind & SOCK_NONBLOCK) != 0 {
        let _ = socket.clone().set_flags(FileFlags::NONBLOCK);
    }
    let fd_flags = if (kind & SOCK_CLOEXEC) != 0 {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(socket, fd_flags);
    Ok(fd)
});

define_syscall!(Socketpair, |domain: u64,
                             kind: u64,
                             protocol: u64,
                             fds: *mut i32| {
    let (left, right) =
        UnixSocketObject::pair(domain, kind, protocol).map_err(ObjectError::from)?;
    let (left_fd, right_fd) = {
        let process = get_current_process();
        let mut process = process.lock();
        let fd_flags = if (kind & SOCK_CLOEXEC) != 0 {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        let left_fd = process.push_object_with_flags(left, fd_flags);
        let right_fd = process.push_object_with_flags(right, fd_flags);
        (left_fd, right_fd)
    };

    let fds_out = [
        i32::try_from(left_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
        i32::try_from(right_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
    ];
    user_safe::write(fds, &fds_out)?;

    Ok(0)
});

define_syscall!(Bind, |socket: ObjectRef,
                       address: *const u8,
                       address_len: u32| {
    let address = socket_address_bytes(address, address_len)?;
    socket
        .clone()
        .as_socket_like()?
        .bind_bytes(&address)
        .map_err(ObjectError::from)?;
    Ok(0)
});

define_syscall!(Listen, |socket: ObjectRef, backlog: usize| {
    socket
        .clone()
        .as_socket_like()?
        .listen(backlog)
        .map_err(ObjectError::from)?;
    Ok(0)
});

define_syscall!(Connect, |socket: ObjectRef,
                          address: *const u8,
                          address_len: u32| {
    let address = socket_address_bytes(address, address_len)?;
    socket
        .clone()
        .as_socket_like()?
        .connect_bytes(&address)
        .map_err(ObjectError::from)?;
    Ok(0)
});

define_syscall!(Accept, |socket: ObjectRef,
                         address: *mut u8,
                         address_len_ptr: *mut u32| {
    let fd = accept_socket(socket, 0)?;
    if !address_len_ptr.is_null() {
        let accepted = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let name = accepted
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
    }
    Ok(fd)
});

define_syscall!(Accept4, |socket: ObjectRef,
                          address: *mut u8,
                          address_len_ptr: *mut u32,
                          flags: u32| {
    let fd = accept_socket(socket, flags)?;
    if !address_len_ptr.is_null() {
        let accepted = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let name = accepted
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
    }
    Ok(fd)
});

define_syscall!(Sendto, |socket: ObjectRef,
                         buffer: *const u8,
                         len: usize,
                         _flags: u64,
                         address: *const u8,
                         address_len: u32| {
    if len > 0 && buffer.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let buffer = if len == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(buffer, len) }
    };
    let address = (!address.is_null())
        .then(|| socket_address_bytes(address, address_len))
        .transpose()?;
    let written = socket
        .as_socket_like()?
        .sendto(buffer, address.as_deref())
        .map_err(ObjectError::from)?;

    Ok(written)
});

define_syscall!(
    Recvfrom,
    |socket: ObjectRef,
     buffer: *mut u8,
     len: usize,
     flags: u64,
     address: *mut u8,
     address_len_ptr: *mut u32| {
        systemd_perf::profile_current_process(PerfBucket::Recvfrom, || {
            if len > 0 && buffer.is_null() {
                return Err(SyscallError::BadAddress);
            }

            if let Ok(socket) = socket.clone().as_netlink_socket()
                && (flags & (MSG_PEEK | MSG_TRUNC)) != 0
            {
                let peek = (flags & MSG_PEEK) != 0;
                let report_trunc = (flags & MSG_TRUNC) != 0;
                let message_len = socket.peek_message_len().ok_or(SyscallError::TryAgain)?;
                let mut data = vec![0; len];
                let (copied, full_len, source, _, _) = socket
                    .recv_message(&mut data, peek)
                    .map_err(SyscallError::from)?;

                if copied > 0 {
                    user_safe::write(buffer, &data[..copied])?;
                }

                if !address.is_null() {
                    if address_len_ptr.is_null() {
                        return Err(SyscallError::BadAddress);
                    }
                    let name = LinuxSockAddrNl {
                        nl_family: AF_NETLINK as u16,
                        nl_pad: 0,
                        nl_pid: source.pid,
                        nl_groups: source.groups,
                    };
                    let requested_len = unsafe { *address_len_ptr as usize };
                    let name_bytes = unsafe {
                        core::slice::from_raw_parts(
                            (&name as *const LinuxSockAddrNl).cast::<u8>(),
                            core::mem::size_of::<LinuxSockAddrNl>(),
                        )
                    };
                    let copy_len = requested_len.min(name_bytes.len());
                    if copy_len > 0 {
                        user_safe::write(address, &name_bytes[..copy_len])?;
                    }
                    user_safe::write(address_len_ptr, &(name_bytes.len() as u32))?;
                }

                return Ok(if report_trunc || len == 0 {
                    full_len.max(message_len)
                } else {
                    copied
                });
            }

            let mut data = vec![0; len];
            let (read, source) = socket
                .as_socket_like()?
                .recvfrom(&mut data)
                .map_err(ObjectError::from)?;

            if read > 0 {
                user_safe::write(buffer, &data[..read])?;
            }

            if !address.is_null() {
                write_socket_name(address, address_len_ptr, &source.unwrap_or_default())?;
            }

            Ok(read)
        })
    }
);

fn sendmsg_rights(msg: &relibc_msg_hdr) -> Result<Vec<ObjectRef>, SyscallError> {
    if msg.msg_controllen == 0 {
        return Ok(Vec::new());
    }
    if msg.msg_control.is_null() || msg.msg_controllen < mem::size_of::<LinuxCmsgHdr>() {
        return Err(SyscallError::BadAddress);
    }

    let header = unsafe { &*(msg.msg_control as *const LinuxCmsgHdr) };
    if header.cmsg_level != SOL_SOCKET as i32 || header.cmsg_type != SCM_RIGHTS {
        return Ok(Vec::new());
    }

    let header_space = cmsg_align(mem::size_of::<LinuxCmsgHdr>());
    if header.cmsg_len < header_space || header.cmsg_len > msg.msg_controllen {
        return Err(SyscallError::InvalidArguments);
    }

    let payload_len = header.cmsg_len - header_space;
    if !payload_len.is_multiple_of(mem::size_of::<i32>()) {
        return Err(SyscallError::InvalidArguments);
    }

    let fd_count = payload_len / mem::size_of::<i32>();
    let fds =
        unsafe { slice::from_raw_parts(msg.msg_control.add(header_space) as *const i32, fd_count) };
    let mut rights = Vec::with_capacity(fd_count);
    for &fd in fds {
        if fd < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        rights.push(get_object_current_process(fd as u64).map_err(SyscallError::from)?);
    }
    Ok(rights)
}

fn sendmsg_impl(
    socket: ObjectRef,
    msg: &relibc_msg_hdr,
    flags: u64,
) -> Result<usize, SyscallError> {
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = if msg.msg_iovlen == 0 {
        &[][..]
    } else {
        if msg.msg_iov.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts(msg.msg_iov, msg.msg_iovlen) }
    };

    if let Ok(socket) = socket.clone().as_netlink_socket() {
        let destination = if !msg.msg_name.is_null() {
            let SocketAddress::Netlink(address) =
                socket_address_from_raw(msg.msg_name.cast(), msg.msg_namelen)?
            else {
                return Err(SyscallError::InvalidArguments);
            };
            Some(address)
        } else {
            None
        };

        let total_len = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut buffer = Vec::with_capacity(total_len);
        for iov in iovs {
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let chunk =
                unsafe { core::slice::from_raw_parts(iov.iov_base.cast_const(), iov.iov_len) };
            buffer.extend_from_slice(chunk);
        }

        let written = socket
            .send(buffer.as_slice(), destination)
            .map_err(ObjectError::from)?;
        return Ok(written);
    }

    if let Ok(socket) = socket.clone().as_inet_socket() {
        let target_addr = if !msg.msg_name.is_null() {
            let SocketAddress::Inet(address) =
                socket_address_from_raw(msg.msg_name.cast(), msg.msg_namelen)?
            else {
                return Err(SyscallError::InvalidArguments);
            };
            Some(address)
        } else {
            None
        };

        if msg.msg_controllen != 0 && !msg.msg_control.is_null() {
            let _ = unsafe { slice::from_raw_parts(msg.msg_control, msg.msg_controllen) };
        }

        let total_len = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut buffer = Vec::with_capacity(total_len);
        for iov in iovs {
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let chunk =
                unsafe { core::slice::from_raw_parts(iov.iov_base.cast_const(), iov.iov_len) };
            buffer.extend_from_slice(chunk);
        }

        let written = match target_addr {
            Some(address) => socket
                .send_to(&buffer, address)
                .map_err(ObjectError::from)?,
            None => socket.send(&buffer).map_err(ObjectError::from)?,
        };
        return Ok(written);
    }

    let target_path = if !msg.msg_name.is_null() {
        let address_len = msg.msg_namelen;
        let SocketAddress::Unix(path) = socket_address_from_raw(msg.msg_name.cast(), address_len)?
        else {
            return Err(SyscallError::InvalidArguments);
        };
        Some(path)
    } else {
        None
    };

    let socket = socket.as_unix_socket()?;
    let dontwait = (flags & MSG_DONTWAIT) != 0;
    let rights = sendmsg_rights(msg)?;
    if socket.kind == UnixSocketKind::Datagram {
        let total_len = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut buffer = Vec::with_capacity(total_len);
        for iov in iovs {
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let chunk =
                unsafe { core::slice::from_raw_parts(iov.iov_base.cast_const(), iov.iov_len) };
            buffer.extend_from_slice(chunk);
        }
        let written = if let Some(path) = target_path.as_deref() {
            socket
                .write_socket_to_path(&buffer, path)
                .map_err(ObjectError::from)?
        } else {
            socket
                .write_socket_with_flags(&buffer, dontwait)
                .map_err(ObjectError::from)?
        };
        return Ok(written);
    }

    if let Some(path) = target_path
        && matches!(&*socket.state.lock(), UnixSocketState::Unbound)
    {
        socket.connect(path).map_err(ObjectError::from)?;
    }

    let total_len = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
    let mut buffer = Vec::with_capacity(total_len);
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let chunk = unsafe { core::slice::from_raw_parts(iov.iov_base.cast_const(), iov.iov_len) };
        buffer.extend_from_slice(chunk);
    }

    socket
        .write_socket_with_rights(&buffer, dontwait, rights)
        .map_err(ObjectError::from)
        .map_err(Into::into)
}

define_syscall!(Sendmsg, |socket: ObjectRef,
                          msg: *const relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    sendmsg_impl(socket, unsafe { &*msg }, flags)
});

define_syscall!(Sendmmsg, |socket: ObjectRef,
                           msgvec: *mut relibc_mmsghdr,
                           vlen: u32,
                           flags: u32| {
    if vlen > 0 && msgvec.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let messages = if vlen == 0 {
        &mut [][..]
    } else {
        unsafe { core::slice::from_raw_parts_mut(msgvec, vlen as usize) }
    };
    let mut sent = 0usize;

    for message in messages {
        match sendmsg_impl(socket.clone(), &message.msg_hdr, flags as u64) {
            Ok(written) => {
                message.msg_len =
                    u32::try_from(written).map_err(|_| SyscallError::InvalidArguments)?;
                sent += 1;
            }
            Err(_) if sent > 0 => break,
            Err(err) => return Err(err),
        }
    }

    Ok(sent)
});

define_syscall!(Setsockopt, |socket: ObjectRef,
                             level: i32,
                             option_name: i32,
                             option_value: *const u8,
                             option_len: u32| {
    if option_len > 0 && option_value.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let option_value = if option_len == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(option_value, option_len as usize) }
    };
    socket
        .as_socket_like()?
        .setsockopt(level as u64, option_name as u64, option_value)
        .map_err(ObjectError::from)?;

    Ok(0)
});

define_syscall!(
    Getsockopt,
    |socket: ObjectRef,
     level: i32,
     option_name: i32,
     option_value: *mut u8,
     option_len_ptr: *mut u32| {
        if option_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let option_len = unsafe { *option_len_ptr as usize };
        let value = socket
            .as_socket_like()?
            .getsockopt(level as u64, option_name as u64, option_len)
            .map_err(ObjectError::from)?;

        if option_value.is_null() {
            if option_len != 0 && !value.is_empty() {
                return Err(SyscallError::BadAddress);
            }
        } else if !value.is_empty() {
            let copy_len = option_len.min(value.len());
            user_safe::write(option_value, &value[..copy_len])?;
        }

        if option_value.is_null() && option_len == 0 {
            user_safe::write(option_len_ptr, &(value.len() as u32))?;
            return Ok(0);
        }

        if option_value.is_null() && value.is_empty() {
            user_safe::write(option_len_ptr, &(value.len() as u32))?;
            return Ok(0);
        }

        if option_value.is_null() && option_len != 0 {
            return Err(SyscallError::BadAddress);
        }
        user_safe::write(option_len_ptr, &(value.len() as u32))?;

        Ok(0)
    }
);

define_syscall!(
    Getsockname,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        let name = socket
            .as_socket_like()?
            .getsockname_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
        Ok(0)
    }
);

define_syscall!(
    Getpeername,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        let name = socket
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
        Ok(0)
    }
);

define_syscall!(Recvmsg, |socket: ObjectRef,
                          msg: *mut relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = unsafe { &mut *msg };
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = if msg.msg_iovlen == 0 {
        &[][..]
    } else {
        if msg.msg_iov.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts_mut(msg.msg_iov, msg.msg_iovlen) }
    };

    if let Ok(socket) = socket.clone().as_netlink_socket() {
        let peek = (flags & MSG_PEEK) != 0;
        let report_trunc = (flags & MSG_TRUNC) != 0;
        let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let message_len = socket.peek_message_len().ok_or(SyscallError::TryAgain)?;
        let mut scratch = alloc::vec![0u8; total_capacity];
        let (copied, full_len, source, uid, gid) = socket
            .recv_message(&mut scratch, peek)
            .map_err(SyscallError::from)?;
        let mut copied_total = 0usize;

        for iov in iovs {
            if copied_total >= copied {
                break;
            }
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }

            let chunk_len = (copied - copied_total).min(iov.iov_len);
            user_safe::write(
                iov.iov_base,
                &scratch[copied_total..copied_total + chunk_len],
            )?;
            copied_total += chunk_len;
        }

        msg.msg_flags = 0;
        if !msg.msg_name.is_null() {
            let name = LinuxSockAddrNl {
                nl_family: AF_NETLINK as u16,
                nl_pad: 0,
                nl_pid: source.pid,
                nl_groups: source.groups,
            };
            let requested_len = msg.msg_namelen as usize;
            let name_bytes = unsafe {
                core::slice::from_raw_parts(
                    (&name as *const LinuxSockAddrNl).cast::<u8>(),
                    core::mem::size_of::<LinuxSockAddrNl>(),
                )
            };
            let copy_len = requested_len.min(name_bytes.len());
            if copy_len > 0 {
                user_safe::write(msg.msg_name.cast::<u8>(), &name_bytes[..copy_len])?;
            }
            msg.msg_namelen = name_bytes.len() as u32;
        }
        let control = netlink_socket_control_bytes(&socket, source.pid, uid, gid)?;
        if control.is_empty() {
            msg.msg_controllen = 0;
        } else if msg.msg_control.is_null() || msg.msg_controllen == 0 {
            msg.msg_flags |= MSG_CTRUNC;
            msg.msg_controllen = 0;
        } else {
            let copy_len = msg.msg_controllen.min(control.len());
            user_safe::write(msg.msg_control, &control[..copy_len])?;
            msg.msg_controllen = copy_len;
            if copy_len < control.len() {
                msg.msg_flags |= MSG_CTRUNC;
            }
        }
        return Ok(if report_trunc || total_capacity == 0 {
            full_len.max(message_len)
        } else {
            copied_total
        });
    }

    if let Ok(socket) = socket.clone().as_inet_socket() {
        let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut scratch = alloc::vec![0u8; total_capacity];
        let (read, source) = socket.recv_from(&mut scratch).map_err(ObjectError::from)?;
        let mut copied_total = 0usize;

        for iov in iovs {
            if copied_total >= read {
                break;
            }
            if iov.iov_len == 0 {
                continue;
            }
            if iov.iov_base.is_null() {
                return Err(SyscallError::BadAddress);
            }

            let chunk_len = (read - copied_total).min(iov.iov_len);
            user_safe::write(
                iov.iov_base,
                &scratch[copied_total..copied_total + chunk_len],
            )?;
            copied_total += chunk_len;
        }

        msg.msg_flags = 0;
        if !msg.msg_name.is_null() {
            let name = source
                .map(|address| socket_address_to_bytes(SocketAddress::Inet(address)))
                .transpose()?
                .unwrap_or_default();
            let copy_len = (msg.msg_namelen as usize).min(name.len());
            if copy_len > 0 {
                user_safe::write(msg.msg_name.cast::<u8>(), &name[..copy_len])?;
            }
            msg.msg_namelen = name.len() as u32;
        } else {
            msg.msg_namelen = 0;
        }
        msg.msg_controllen = 0;
        return Ok(copied_total);
    }

    let socket = socket.as_unix_socket()?;
    let dontwait = (flags & MSG_DONTWAIT) != 0;
    let peek = (flags & MSG_PEEK) != 0;
    let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
    let mut scratch = alloc::vec![0u8; total_capacity];
    let total_read = socket
        .read_socket_with_flags_and_mode(&mut scratch, dontwait, peek)
        .map_err(ObjectError::from)?;

    let mut copied_total = 0usize;
    for iov in iovs {
        if copied_total >= total_read {
            break;
        }
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let chunk_len = (total_read - copied_total).min(iov.iov_len);
        user_safe::write(
            iov.iov_base,
            &scratch[copied_total..copied_total + chunk_len],
        )?;
        copied_total += chunk_len;
    }

    msg.msg_flags = 0;
    if !msg.msg_name.is_null() {
        let name = socket.getpeername_bytes().map_err(ObjectError::from)?;
        let copy_len = (msg.msg_namelen as usize).min(name.len());
        if copy_len > 0 {
            user_safe::write(msg.msg_name.cast::<u8>(), &name[..copy_len])?;
        }
        msg.msg_namelen = name.len() as u32;
    } else {
        msg.msg_namelen = 0;
    }
    let control = if total_read > 0 {
        unix_socket_control_bytes(&socket, total_read, peek)?
    } else {
        Vec::new()
    };
    if control.is_empty() {
        msg.msg_controllen = 0;
    } else if msg.msg_control.is_null() || msg.msg_controllen == 0 {
        msg.msg_flags |= MSG_CTRUNC;
        msg.msg_controllen = 0;
    } else {
        let copy_len = msg.msg_controllen.min(control.len());
        user_safe::write(msg.msg_control, &control[..copy_len])?;
        msg.msg_controllen = copy_len;
        if copy_len < control.len() {
            msg.msg_flags |= MSG_CTRUNC;
        }
    }

    Ok(total_read)
});

define_syscall!(Shutdown, |socket: ObjectRef, how: u64| {
    socket
        .as_socket_like()?
        .shutdown(how)
        .map_err(ObjectError::from)?;
    Ok(0)
});

fn socket_address_to_bytes(address: SocketAddress) -> Result<Vec<u8>, SyscallError> {
    match address {
        SocketAddress::Inet(address) => {
            let sockaddr = LinuxSockAddrIn {
                sin_family: AF_INET as u16,
                sin_port: address.port.to_be(),
                sin_addr: address.addr,
                sin_zero: [0; 8],
            };
            Ok(unsafe {
                slice::from_raw_parts(
                    (&sockaddr as *const LinuxSockAddrIn).cast::<u8>(),
                    mem::size_of::<LinuxSockAddrIn>(),
                )
            }
            .to_vec())
        }
        SocketAddress::Unix(_) | SocketAddress::Netlink(_) => Err(SyscallError::InvalidArguments),
    }
}

#[repr(C)]
struct relibc_iovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
struct relibc_msg_hdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    msg_iov: *mut relibc_iovec,
    msg_iovlen: usize,
    msg_control: *mut u8,
    msg_controllen: usize,
    msg_flags: i32,
}

#[repr(C)]
struct relibc_mmsghdr {
    msg_hdr: relibc_msg_hdr,
    msg_len: u32,
}
