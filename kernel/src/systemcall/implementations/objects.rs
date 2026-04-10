use alloc::{collections::btree_map::BTreeMap, string::String, vec};
use seele_sys::{
    abi::object::{ControlCommand, SeekType},
    permission::Permissions,
};
use spin::Mutex;

use crate::{
    define_syscall,
    filesystem::vfs_traits::DirectoryContentType,
    misc::c_types::CString,
    object::{
        config::ConfigurateRequest,
        control::control_object,
        device::get_device,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{
        manager::get_current_process,
        misc::{ProcessID, with_current_process},
    },
    misc::usercopy::{copy_from_user, copy_to_user},
    systemcall::utils::{SyscallError, SyscallImpl},
};

static DIR_OFFSETS: Mutex<BTreeMap<(ProcessID, u64), usize>> = Mutex::new(BTreeMap::new());

define_syscall!(
    GetDirectoryContents,
    |object_index: u64, buf: *mut u8, len: usize| {
        let obj = get_object_current_process(object_index)?.as_file_like()?;
        let contents = obj.directory_contents()?;
        let current_pid = get_current_process().lock().pid;
        let mut offsets = DIR_OFFSETS.lock();
        let offset_entry = offsets.entry((current_pid, object_index)).or_insert(0usize);
        let mut output = vec![0u8; len];
        let mut bytes_written = 0usize;

        while *offset_entry < contents.len() {
            let info = &contents[*offset_entry];
            let name_bytes = info.name.as_bytes();
            let reclen = ((20 + name_bytes.len() + 7) & !7) as u16;
            if bytes_written + reclen as usize > len {
                break;
            }

            {
                let entry_ptr = unsafe { output.as_mut_ptr().add(bytes_written) };
                unsafe {
                    entry_ptr.cast::<u64>().write_unaligned(1);
                    entry_ptr
                        .add(8)
                        .cast::<i64>()
                        .write_unaligned((*offset_entry as i64) + 1);
                    entry_ptr.add(16).cast::<u16>().write_unaligned(reclen);
                }
                let linux_type = match info.content_type {
                    DirectoryContentType::Directory => 4,
                    DirectoryContentType::File => 8,
                    _ => 0,
                };
                unsafe {
                    entry_ptr.add(18).write(linux_type);
                    core::ptr::copy_nonoverlapping(
                        name_bytes.as_ptr(),
                        entry_ptr.add(19),
                        name_bytes.len(),
                    );
                    entry_ptr.add(19 + name_bytes.len()).write(0);
                }
            }

            bytes_written += reclen as usize;
            *offset_entry += 1;
        }

        if *offset_entry >= contents.len() && bytes_written == 0 {
            offsets.remove(&(current_pid, object_index));
            return Ok(0);
        }

        if !copy_to_user(buf, &output[..bytes_written]) {
            return Err(SyscallError::BadAddress);
        }

        Ok(bytes_written)
    }
);

define_syscall!(ReadObject, |object: ObjectRef,
                             buf_ptr: *mut u8,
                             len: usize| {
    let mut buffer = vec![0u8; len];
    let read = object.as_readable()?.read(&mut buffer)?;
    if !copy_to_user(buf_ptr, &buffer[..read]) {
        return Err(SyscallError::BadAddress);
    }
    Ok(read)
});

define_syscall!(WriteObject, |object: ObjectRef,
                              buf_ptr: *mut u8,
                              len: usize| {
    let mut buffer = vec![0u8; len];
    if !copy_from_user(buf_ptr, &mut buffer) {
        return Err(SyscallError::BadAddress);
    }
    Ok(object.as_writable()?.write(&buffer)?)
});

define_syscall!(RemoveObject, |object_num: usize| {
    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let current_pid = process.pid;
    let objects = &mut process.objects;

    if objects.len() > object_num {
        let object = objects[object_num].take();
        drop(object);
        DIR_OFFSETS.lock().remove(&(current_pid, object_num as u64));
        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(
    ConfigurateObject,
    |object: ObjectRef, request: u64, request_ptr: u64| {
        let res = object
            .as_configuratable()?
            .configure(ConfigurateRequest::new(request, request_ptr)?);

        res.map(|val| val as usize).map_err(Into::into)
    }
);

define_syscall!(ControlObject, |object: ObjectRef,
                                command: u64,
                                arg: u64| {
    control_object(object, command, arg)
});

define_syscall!(CloneObject, |object: ObjectRef| {
    get_current_process()
        .lock()
        .clone_object(object)
        .map_err(Into::into)
});

define_syscall!(CloneObjectTo, |source: ObjectRef, dest: usize| {
    get_current_process()
        .lock()
        .clone_object_to(source, dest)
        .map_err(Into::into)
});

define_syscall!(OpenDevice, |name: String| {
    with_current_process(|process| {
        let device = get_device(name)?;
        let slot = process.push_object(device);

        Ok(slot)
    })
});

define_syscall!(
    MmapObject,
    |object: ObjectRef, pages: u64, offset: u64, permissions: Permissions| {
        let object = object.as_mappable()?;
        let address = object.map(offset, pages, permissions)?;

        Ok(address.as_u64() as usize)
    }
);

define_syscall!(
    SeekObject,
    |object: ObjectRef, offset: i64, seek_type: SeekType| {
        object
            .as_seekable()?
            .seek(offset, seek_type)
            .map_err(Into::into)
    }
);
