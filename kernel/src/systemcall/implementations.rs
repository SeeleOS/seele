use core::slice;

use acpi::aml::object;
use alloc::{
    collections::{btree_map::BTreeMap, vec_deque::VecDeque},
    ffi::CString,
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;
use x86_64::{VirtAddr, registers::model_specific::FsBase, structures::paging::Page};

use crate::{
    filesystem::{info::LinuxStat, path::Path, vfs::VirtualFS, vfs_traits::DirectoryContentType},
    multitasking::{
        MANAGER,
        process::{
            ProcessRef,
            execve::{self, execve},
            manager::get_current_process,
            misc::ProcessID,
        },
        scheduling::{return_to_executor_from_current, return_to_executor_no_save},
        thread::{
            THREAD_MANAGER,
            misc::State,
            yielding::{BlockType, WakeType, block_current},
        },
    },
    object::{config::ConfigurateRequest, misc::get_object_current_process},
    systemcall::{error::SyscallError, numbers::SyscallNo, utils::SyscallImpl},
};

use crate::define_syscall;

static FUTEX_QUEUE: Mutex<BTreeMap<u64, VecDeque<ProcessRef>>> = Mutex::new(BTreeMap::new());

define_syscall!(
    GetDirectoryContents,
    |object: u64, buf: *mut u8, len: usize| {
        // 1. 获取 Object 并调用你写好的原生接口
        let obj = get_object_current_process(object)
            .ok_or(SyscallError::BadFileDescriptor)?
            .as_file_like()
            .ok_or(SyscallError::BadFileDescriptor)?;

        let contents = obj.directory_contents()?;

        let mut bytes_written = 0;

        // 2. 遍历你拿到的 Vec<DirectoryContentInfo>
        for info in contents {
            let name_bytes = info.name.as_bytes();
            let name_len = name_bytes.len();

            // 计算 Linux 要求的长度:
            // 8(ino) + 8(off) + 2(reclen) + 1(type) + name_len + 1(\0) = 19 + name_len + 1
            let base_len = 19 + name_len + 1;

            // 关键：必须 8 字节对齐，否则 ls 直接 Invalid Argument
            let reclen = ((base_len + 7) & !7) as u16;

            // 检查缓冲区是否溢出
            if bytes_written + (reclen as usize) > len {
                break;
            }

            unsafe {
                let entry_ptr = buf.add(bytes_written);

                // 填充 Inode (如果没有真实的，暂时填 1)
                entry_ptr.cast::<u64>().write_unaligned(1);

                // 填充 d_off (通常是目录流的逻辑位置)
                entry_ptr.add(8).cast::<i64>().write_unaligned(0);

                // 填充 d_reclen
                entry_ptr.add(16).cast::<u16>().write_unaligned(reclen);

                // 转换类型 (Seele Enum -> Linux Type)
                let linux_type = match info.content_type {
                    DirectoryContentType::Directory => 4, // DT_DIR
                    DirectoryContentType::File => 8,      // DT_REG
                    _ => 0,                               // DT_UNKNOWN
                };
                entry_ptr.add(18).write(linux_type);

                // 填充文件名
                core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), entry_ptr.add(19), name_len);
                // 补上 \0
                entry_ptr.add(19 + name_len).write(0);
            }

            bytes_written += reclen as usize;
        }

        Ok(bytes_written)
    }
);

define_syscall!(
    WaitForProcessExit,
    |target_process: ProcessID, exit_code_ptr: *mut u64| {
        let check_result = {
            let manager = MANAGER.lock();
            let process = manager
                .processes
                .iter()
                .find(|(pid, _)| **pid == target_process)
                .ok_or(SyscallError::NoProcess)?;

            let p_lock = process.1.lock();
            if p_lock.threads.is_empty() {
                let code = p_lock.exit_code.unwrap_or(0);
                Some(code)
            } else {
                None
            }
        };

        match check_result {
            Some(exit_code) => {
                if !exit_code_ptr.is_null() {
                    unsafe {
                        *exit_code_ptr = exit_code;
                    }
                }
                return Ok(0);
            }
            None => {
                return Err(SyscallError::TryAgain);
            }
        }
    }
);

define_syscall!(FutexWait, |arg1: u64, arg2: u64| {
    let mut queue = FUTEX_QUEUE.lock();
    let cur_value = unsafe { *(arg1 as *mut u64) };

    if cur_value != arg2 {
        return Err(SyscallError::TryAgain);
    }

    if !queue.contains_key(&arg1) {
        queue.insert(arg1, VecDeque::new());
    }

    queue
        .get_mut(&arg1)
        .unwrap()
        .push_back(MANAGER.lock().current.clone().unwrap());

    drop(queue);

    //block_current(BlockType::Futex);
    Ok(0)
});

define_syscall!(FutexWake, |arg1: u64, arg2: u64| {
    let mut queue = FUTEX_QUEUE.lock();
    let mut woken = 0;

    if let Some(queue) = queue.get_mut(&arg1) {
        for _ in 0..arg2 {
            if let Some(_process) = queue.pop_front() {
                //MANAGER.lock().wake(process);
                log::warn!("[TODO] Futex wake not implemented");
                woken += 1;
            } else {
                break;
            }
        }
    }

    Ok(woken)
});

define_syscall!(SetGs, { Err(SyscallError::other("setgs unimplemented")) });

define_syscall!(SetFs, |fs: u64| {
    FsBase::write(VirtAddr::new(fs));
    Ok(0)
});

define_syscall!(OpenFile, |path_str: String| {
    let path = Path::new(path_str.as_str());

    let object = Arc::new(VirtualFS.lock().open(path)?);

    let current_process = get_current_process();
    current_process.lock().objects.push(Some(object));
    Ok(current_process.lock().objects.len() - 1)
});

define_syscall!(AllocateMem, |pages: u64| {
    let current = get_current_process();

    let (mem_start, _) = current.lock().addrspace.allocate_user(pages);
    Ok(mem_start.as_u64() as usize)
});

define_syscall!(
    ConfigurateObject,
    |object: u64, request: u64, request_ptr: u64| {
        let res = get_object_current_process(object)
            .ok_or(SyscallError::BadFileDescriptor)?
            .as_configuratable()
            .ok_or(SyscallError::InappropriateIoctl)?
            .configure(ConfigurateRequest::new(request, request_ptr));

        match res {
            Ok(val) => Ok(val as usize),
            Err(_) => {
                log::warn!("ConfigurateObject failed; returning Ok(0)");
                Ok(0)
            }
        }
    }
);

define_syscall!(ChangeDirectory, |dir: String| {
    get_current_process()
        .lock()
        .change_directory(Path::new(&dir))?;
    Ok(0)
});

define_syscall!(GetCurrentDirectory, |buf_ptr: *mut u8, len: usize| {
    let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, len) };

    let process = get_current_process();
    let path_str = process.lock().current_directory.clone().as_string().clone();
    let path_bytes = path_str.as_bytes();

    let path_len = path_bytes.len();

    if len > path_len {
        // only copy the needed part
        buf[..path_len].copy_from_slice(path_bytes);

        // add \0
        buf[path_len] = 0;
    } else {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(buf_ptr as usize)
});

define_syscall!(Execve, |path_str: String,
                         args: Vec<String>,
                         env: Vec<String>| {
    let path = Path::new(path_str.as_str());

    execve(path, args, env)?;
    log::info!("execve done");

    Ok(0)
});

define_syscall!(Exit, |exit_code: u64| {
    let mut manager = THREAD_MANAGER.get().unwrap().lock();

    log::debug!(
        "exit: pid {} code {}",
        get_current_process().lock().pid.0,
        exit_code
    );
    manager.mark_current_as_zombie();

    get_current_process().lock().exit_code = Some(exit_code);

    drop(manager);

    return_to_executor_no_save();

    panic!("What the fuck")
});

define_syscall!(FileInfo, |start_from_current_dir: bool,
                           path_str: String,
                           linux_stat_ptr: *mut LinuxStat,
                           use_object: bool,
                           object: u64| {
    let path: Path;
    if !use_object {
        if path_str.starts_with('/') {
            path = Path::new(&path_str);
        } else {
            if start_from_current_dir {
                // start from current directory
                path = Path::new(
                    (get_current_process().lock().current_directory.1.clone() + &path_str).as_str(),
                );
            } else {
                return Err(SyscallError::other(
                    "Non-absolute paths are not supported yet",
                ));
            }
        }
    } else {
        unsafe {
            *linux_stat_ptr = get_current_process()
                .lock()
                .get_object(object)?
                .as_file_like()
                .ok_or(SyscallError::InvalidArguments)?
                .info()?
                .as_linux()
        };

        return Ok(0);
    }

    let info = VirtualFS.lock().file_info(path).unwrap();

    unsafe { *linux_stat_ptr = info.as_linux() };

    Ok(0)
});

define_syscall!(Fork, {
    let mut manager = MANAGER.lock();

    log::debug!("start fork");
    let current = manager.current.clone().unwrap();
    Ok(current.lock().fork(&mut manager).0 as usize)
});

define_syscall!(GetFs, { Ok(FsBase::read().as_u64() as usize) });

define_syscall!(ReadObject, |object: u64, buf_ptr: *mut u8, len: usize| {
    unsafe {
        Ok(get_object_current_process(object)
            .ok_or(SyscallError::BadFileDescriptor)?
            .as_readable()
            .ok_or(SyscallError::BadFileDescriptor)?
            .read(slice::from_raw_parts_mut(buf_ptr, len))?)
    }
});

define_syscall!(WriteObject, |object: u64, buf_ptr: *mut u8, len: usize| {
    unsafe {
        Ok(get_object_current_process(object)
            .ok_or(SyscallError::BadFileDescriptor)?
            .as_writable()
            .ok_or(SyscallError::BadFileDescriptor)?
            .write(slice::from_raw_parts(buf_ptr, len))?)
    }
});

define_syscall!(RemoveObject, |object: usize| {
    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let objects = &mut process.objects;

    if objects.len() > object {
        let object = objects[object].take();

        drop(object);

        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(GetProcessID, {
    Ok(get_current_process().lock().pid.0 as usize)
});

define_syscall!(GetThreadID, {
    Err(SyscallError::other("get tid unimplemented"))
});
