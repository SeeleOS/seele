use super::*;

#[allow(dead_code)]
pub(super) struct LinuxSockAddrNl {
    pub(super) nl_family: u16,
    pub(super) nl_pad: u16,
    pub(super) nl_pid: u32,
    pub(super) nl_groups: u32,
}

#[allow(dead_code)]
pub(super) struct LinuxSockAddrIn {
    pub(super) sin_family: u16,
    pub(super) sin_port: u16,
    pub(super) sin_addr: [u8; 4],
    pub(super) sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct LinuxCmsgHdr {
    pub(super) cmsg_len: usize,
    pub(super) cmsg_level: i32,
    pub(super) cmsg_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct LinuxUcred {
    pub(super) pid: i32,
    pub(super) uid: u32,
    pub(super) gid: u32,
}
pub(super) const MSG_PEEK: u64 = 0x2;
pub(super) const MSG_CTRUNC: i32 = 0x8;
pub(super) const MSG_CMSG_CLOEXEC: u64 = 0x40000000;
pub(super) const MSG_DONTWAIT: u64 = 0x40;
pub(super) const MSG_TRUNC: u64 = 0x20;
pub(super) const SCM_RIGHTS: i32 = 1;
pub(super) const SCM_CREDENTIALS: i32 = 2;

pub(super) enum SocketAddress {
    Inet(InetAddress),
    Unix(String),
    Netlink(NetlinkSocketAddress),
}

pub(super) fn socket_address_from_raw(
    address: *const u8,
    address_len: u32,
) -> Result<SocketAddress, SyscallError> {
    let bytes = socket_address_bytes(address, address_len)?;
    socket_address_from_bytes(&bytes)
}

pub(super) fn socket_address_from_bytes(address: &[u8]) -> Result<SocketAddress, SyscallError> {
    if address.len() < 2 {
        return Err(SyscallError::InvalidArguments);
    }

    let family = u16::from_ne_bytes(address[..2].try_into().unwrap());
    if family == AF_UNIX as u16 {
        let path = &address[2..];
        let path_len = path.len().min(108);
        if path_len == 0 {
            return Err(SyscallError::InvalidArguments);
        }

        if path[0] == 0 {
            if path_len <= 1 {
                return Err(SyscallError::InvalidArguments);
            }
            return Ok(SocketAddress::Unix(
                String::from_utf8_lossy(&path[..path_len]).into_owned(),
            ));
        }

        let len = path[..path_len]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_len);
        if len == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        return Ok(SocketAddress::Unix(
            String::from_utf8_lossy(&path[..len]).into_owned(),
        ));
    }

    if family == AF_INET as u16 {
        if address.len() < mem::size_of::<LinuxSockAddrIn>() {
            return Err(SyscallError::InvalidArguments);
        }
        let port = u16::from_ne_bytes(address[2..4].try_into().unwrap());
        let addr = [address[4], address[5], address[6], address[7]];
        return Ok(SocketAddress::Inet(InetAddress::new(
            addr,
            u16::from_be(port),
        )));
    }

    if family == AF_NETLINK as u16 {
        if address.len() < mem::size_of::<LinuxSockAddrNl>() {
            return Err(SyscallError::InvalidArguments);
        }
        let pid = u32::from_ne_bytes(address[4..8].try_into().unwrap());
        let groups = u32::from_ne_bytes(address[8..12].try_into().unwrap());
        return Ok(SocketAddress::Netlink(NetlinkSocketAddress { pid, groups }));
    }

    Err(SyscallError::AddressFamilyNotSupported)
}

pub(super) fn write_socket_name(
    address: *mut u8,
    address_len_ptr: *mut u32,
    name: &[u8],
) -> Result<(), SyscallError> {
    if address_len_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let requested_len = user_safe::read(address_len_ptr)? as usize;
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

pub(super) fn socket_address_bytes(
    address: *const u8,
    address_len: u32,
) -> Result<Vec<u8>, SyscallError> {
    if address.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if address_len < 2 {
        return Err(SyscallError::InvalidArguments);
    }
    user_safe::read_buffer(address, address_len as usize)
}

pub(super) fn accept_socket(socket: ObjectRef, flags: u32) -> Result<usize, SyscallError> {
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

pub(super) fn socket_address_to_bytes(address: SocketAddress) -> Result<Vec<u8>, SyscallError> {
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
