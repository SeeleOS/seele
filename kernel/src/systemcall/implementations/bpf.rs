use alloc::{sync::Arc, vec, vec::Vec};
use core::mem::size_of;
use num_enum::TryFromPrimitive;

use crate::{
    define_syscall,
    memory::user_safe,
    object::{Object, bpf::BpfObject, misc::get_object_current_process},
    process::{FdFlags, manager::get_current_process},
    systemcall::utils::{SyscallError, SyscallImpl},
};

const BPF_OBJ_NAME_LEN: usize = 16;
const BPF_PROG_TYPE_UNSPEC: u32 = 0;

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u32)]
enum BpfCommand {
    MapCreate = 0,
    MapLookupElem = 1,
    MapUpdateElem = 2,
    ProgLoad = 5,
    ProgAttach = 8,
    ProgDetach = 9,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfMapCreateAttr {
    map_type: u32,
    key_size: u32,
    value_size: u32,
    max_entries: u32,
    _map_flags: u32,
    _inner_map_fd: u32,
    _numa_node: u32,
    _map_name: [u8; BPF_OBJ_NAME_LEN],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfMapElemAttr {
    map_fd: u32,
    _padding: u32,
    key: u64,
    value: u64,
    _flags: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfProgLoadAttr {
    prog_type: u32,
    insn_cnt: u32,
    insns: u64,
    license: u64,
    _log_level: u32,
    _log_size: u32,
    _log_buf: u64,
    _kern_version: u32,
    _prog_flags: u32,
    _prog_name: [u8; BPF_OBJ_NAME_LEN],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BpfProgAttachAttr {
    target_fd: u32,
    attach_bpf_fd: u32,
    _attach_type: u32,
    _attach_flags: u32,
    _replace_bpf_fd: u32,
    _relative_fd: u32,
    _expected_revision: u64,
}

fn read_bpf_attr<T: Copy>(attr: *const u8, size: usize) -> Result<T, SyscallError> {
    if size < size_of::<T>() {
        return Err(SyscallError::InvalidArguments);
    }

    user_safe::read(attr.cast::<T>())
}

fn read_user_bytes(ptr: *const u8, len: usize) -> Result<Vec<u8>, SyscallError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut bytes = vec![0; len];
    for (offset, slot) in bytes.iter_mut().enumerate() {
        *slot = user_safe::read(unsafe { ptr.add(offset) })?;
    }
    Ok(bytes)
}

fn get_fd_object(fd: u32) -> Result<Arc<dyn Object>, SyscallError> {
    let fd = fd as i32;
    if fd < 0 {
        return Err(SyscallError::BadFileDescriptor);
    }

    get_object_current_process(fd as u64).map_err(|_| SyscallError::BadFileDescriptor)
}

fn create_map(attr: BpfMapCreateAttr) -> Result<usize, SyscallError> {
    if attr.key_size == 0 || attr.value_size == 0 || attr.max_entries == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let fd = get_current_process().lock().push_object_with_flags(
        BpfObject::new_map(
            attr.map_type,
            attr.key_size,
            attr.value_size,
            attr.max_entries,
        ),
        FdFlags::empty(),
    );
    Ok(fd)
}

fn lookup_map_element(attr: BpfMapElemAttr) -> Result<usize, SyscallError> {
    let object = get_fd_object(attr.map_fd)?.as_bpf()?;
    let key = read_user_bytes(attr.key as *const u8, map_key_size(&object)?)?;
    let value = object.lookup_map_element(&key)?;
    user_safe::write(attr.value as *mut u8, &value[..])?;
    Ok(0)
}

fn update_map_element(attr: BpfMapElemAttr) -> Result<usize, SyscallError> {
    let object = get_fd_object(attr.map_fd)?.as_bpf()?;
    let key = read_user_bytes(attr.key as *const u8, map_key_size(&object)?)?;
    let value = read_user_bytes(attr.value as *const u8, map_value_size(&object)?)?;
    object.update_map_element(&key, &value)?;
    Ok(0)
}

fn load_program(attr: BpfProgLoadAttr) -> Result<usize, SyscallError> {
    if attr.prog_type == BPF_PROG_TYPE_UNSPEC || attr.insn_cnt == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if attr.insns == 0 || attr.license == 0 {
        return Err(SyscallError::BadAddress);
    }

    let fd = get_current_process()
        .lock()
        .push_object_with_flags(BpfObject::new_program(attr.prog_type), FdFlags::empty());
    Ok(fd)
}

fn attach_program(attr: BpfProgAttachAttr) -> Result<usize, SyscallError> {
    let _target = get_fd_object(attr.target_fd)?;
    let program = get_fd_object(attr.attach_bpf_fd)?.as_bpf()?;
    let _ = program.prog_type()?;
    Ok(0)
}

fn detach_program(attr: BpfProgAttachAttr) -> Result<usize, SyscallError> {
    let _target = get_fd_object(attr.target_fd)?;
    let program = get_fd_object(attr.attach_bpf_fd)?.as_bpf()?;
    let _ = program.prog_type()?;
    Ok(0)
}

fn map_key_size(object: &Arc<BpfObject>) -> Result<usize, SyscallError> {
    object.map_key_size()
}

fn map_value_size(object: &Arc<BpfObject>) -> Result<usize, SyscallError> {
    object.map_value_size()
}

define_syscall!(Bpf, |cmd: u32, attr: *const u8, size: usize| {
    if attr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    match BpfCommand::try_from(cmd) {
        Ok(BpfCommand::MapCreate) => create_map(read_bpf_attr(attr, size)?),
        Ok(BpfCommand::MapLookupElem) => lookup_map_element(read_bpf_attr(attr, size)?),
        Ok(BpfCommand::MapUpdateElem) => update_map_element(read_bpf_attr(attr, size)?),
        Ok(BpfCommand::ProgLoad) => load_program(read_bpf_attr(attr, size)?),
        Ok(BpfCommand::ProgAttach) => attach_program(read_bpf_attr(attr, size)?),
        Ok(BpfCommand::ProgDetach) => detach_program(read_bpf_attr(attr, size)?),
        Err(_) => Err(SyscallError::InvalidArguments),
    }
});
