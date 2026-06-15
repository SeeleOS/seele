use super::*;

define_syscall!(Setsockopt, |socket: ObjectRef,
                             level: i32,
                             option_name: i32,
                             option_value: *const u8,
                             option_len: u32| {
    if option_len > 0 && option_value.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let option_value = if option_len == 0 {
        Vec::new()
    } else {
        user_safe::read_buffer(option_value, option_len as usize)?
    };
    socket
        .as_socket_like()?
        .setsockopt(level as u64, option_name as u64, option_value.as_slice())
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

        let option_len = user_safe::read(option_len_ptr)? as usize;
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
