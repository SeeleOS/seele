use super::*;

fn sendmsg_rights(msg: &relibc_msg_hdr) -> Result<Vec<ObjectRef>, SyscallError> {
    if msg.msg_controllen == 0 {
        return Ok(Vec::new());
    }
    if msg.msg_control.is_null() || msg.msg_controllen < mem::size_of::<LinuxCmsgHdr>() {
        return Err(SyscallError::BadAddress);
    }

    let control = user_safe::read_buffer(msg.msg_control, msg.msg_controllen)?;
    let header_space = cmsg_align(mem::size_of::<LinuxCmsgHdr>());
    let mut offset = 0usize;
    let mut rights = Vec::new();

    while offset + mem::size_of::<LinuxCmsgHdr>() <= control.len() {
        let header = unsafe { &*(control[offset..].as_ptr().cast::<LinuxCmsgHdr>()) };
        if header.cmsg_len < header_space {
            return Err(SyscallError::InvalidArguments);
        }

        let end = offset + header.cmsg_len;
        if end > control.len() {
            return Err(SyscallError::InvalidArguments);
        }

        if header.cmsg_level == SOL_SOCKET as i32 && header.cmsg_type == SCM_RIGHTS {
            let payload_len = header.cmsg_len - header_space;
            if !payload_len.is_multiple_of(mem::size_of::<i32>()) {
                return Err(SyscallError::InvalidArguments);
            }

            let fd_count = payload_len / mem::size_of::<i32>();
            let fds = unsafe {
                slice::from_raw_parts(
                    control[offset + header_space..].as_ptr().cast::<i32>(),
                    fd_count,
                )
            };
            for &fd in fds {
                if fd < 0 {
                    return Err(SyscallError::InvalidArguments);
                }
                rights.push(get_object_current_process(fd as u64).map_err(SyscallError::from)?);
            }
        }

        let next = cmsg_align(end);
        if next > control.len() {
            if end != control.len() {
                return Err(SyscallError::InvalidArguments);
            }
            break;
        }

        offset = next;
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

    let iovs = read_iovecs_from_user(msg.msg_iov, msg.msg_iovlen)?;

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

        let buffer = copy_iovecs_from_user(&iovs)?;

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
            let _ = user_safe::read_buffer(msg.msg_control, msg.msg_controllen)?;
        }

        let buffer = copy_iovecs_from_user(&iovs)?;

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
        let buffer = copy_iovecs_from_user(&iovs)?;
        let written = if let Some(path) = target_path.as_deref() {
            socket
                .write_socket_to_path_with_rights(&buffer, path, dontwait, rights)
                .map_err(ObjectError::from)?
        } else {
            socket
                .write_socket_with_rights(&buffer, dontwait, rights)
                .map_err(ObjectError::from)?
        };
        return Ok(written);
    }

    if let Some(path) = target_path
        && matches!(&*socket.state.lock(), UnixSocketState::Unbound)
    {
        socket.connect(path).map_err(ObjectError::from)?;
    }

    let buffer = copy_iovecs_from_user(&iovs)?;

    socket
        .write_socket_with_rights(&buffer, dontwait, rights)
        .map_err(ObjectError::from)
        .map_err(Into::into)
}

define_syscall!(Sendto, |socket: ObjectRef,
                         buffer: *const u8,
                         len: usize,
                         _flags: u64,
                         address: *const u8,
                         address_len: u32| {
    if len > 0 && buffer.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let user_buffer = if len == 0 {
        Vec::new()
    } else {
        user_safe::read_buffer(buffer, len)?
    };
    let address = (!address.is_null())
        .then(|| socket_address_bytes(address, address_len))
        .transpose()?;
    let written = socket
        .as_socket_like()?
        .sendto(user_buffer.as_slice(), address.as_deref())
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
                let requested_len = user_safe::read(address_len_ptr)? as usize;
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
            .clone()
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
    }
);

define_syscall!(Sendmsg, |socket: ObjectRef,
                          msg: *const relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = user_safe::read(msg)?;
    sendmsg_impl(socket, &msg, flags)
});

define_syscall!(Sendmmsg, |socket: ObjectRef,
                           msgvec: *mut relibc_mmsghdr,
                           vlen: u32,
                           flags: u32| {
    if vlen > 0 && msgvec.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut sent = 0usize;

    for index in 0..(vlen as usize) {
        let message_ptr = unsafe { msgvec.add(index) };
        let mut message = user_safe::read(message_ptr)?;
        match sendmsg_impl(socket.clone(), &message.msg_hdr, flags as u64) {
            Ok(written) => {
                message.msg_len =
                    u32::try_from(written).map_err(|_| SyscallError::InvalidArguments)?;
                user_safe::write(message_ptr, &message)?;
                sent += 1;
            }
            Err(_) if sent > 0 => break,
            Err(err) => return Err(err),
        }
    }

    Ok(sent)
});

define_syscall!(Recvmsg, |socket: ObjectRef,
                          msg: *mut relibc_msg_hdr,
                          flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg_ptr = msg;
    let mut msg = user_safe::read(msg_ptr)?;
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = read_iovecs_from_user(msg.msg_iov.cast_const(), msg.msg_iovlen)?;

    let result = (|| {
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
        let report_trunc = (flags & MSG_TRUNC) != 0;
        let total_capacity = iovs.iter().map(|iov| iov.iov_len).sum::<usize>();
        let mut scratch =
            alloc::vec![0u8; unix_seqpacket_next_len(&socket).unwrap_or(total_capacity)];
        let total_read = socket
            .recv_socket_with_flags_and_mode(&mut scratch, dontwait, peek)
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
        if copied_total < total_read {
            msg.msg_flags |= MSG_TRUNC as i32;
        }
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
        let control = if total_read > 0
            || socket.kind != UnixSocketKind::Stream
            || unix_stream_has_front_rights(&socket)
        {
            unix_socket_control_bytes(&socket, total_read, peek, flags, msg.msg_controllen)?
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

        Ok(if report_trunc || total_capacity == 0 {
            total_read
        } else {
            copied_total
        })
    })();

    if result.is_ok() {
        user_safe::write(msg_ptr, &msg)?;
    }
    result
});
