use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::mem;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, FrameDeallocator, PageTableFlags, PhysFrame, Size4KiB},
};

use crate::{
    memory::{
        addrspace::{
            cow::{decrease_ref, increase_ref},
            mem_area::{Data, MemoryArea},
        },
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    misc::time::unix_timestamp_seconds,
    process::Process,
    systemcall::utils::{SyscallError, SyscallResult},
};

const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0o1000;
const IPC_EXCL: i32 = 0o2000;
const IPC_RMID: i32 = 0;
const IPC_STAT: i32 = 2;
const SHM_RDONLY: i32 = 0o10000;
const SHM_RND: i32 = 0o20000;
const SHMLBA: u64 = 4096;
const PAGE_SIZE: u64 = 4096;
const IPC_MODE_MASK: i32 = 0o777;

lazy_static! {
    static ref SYSV_SHM_STATE: Mutex<SysvShmState> = Mutex::new(SysvShmState::default());
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxIpcPerm {
    pub __ipc_perm_key: i32,
    pub uid: u32,
    pub gid: u32,
    pub cuid: u32,
    pub cgid: u32,
    pub mode: u32,
    pub __ipc_perm_seq: i32,
    pub __pad1: i64,
    pub __pad2: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxShmidDs {
    pub shm_perm: LinuxIpcPerm,
    pub shm_segsz: usize,
    pub shm_atime: i64,
    pub shm_dtime: i64,
    pub shm_ctime: i64,
    pub shm_cpid: i32,
    pub shm_lpid: i32,
    pub shm_nattch: u64,
    pub __pad1: u64,
    pub __pad2: u64,
}

#[derive(Clone, Debug)]
pub struct ProcessShmMapping {
    pub shmid: i32,
    pub addr: VirtAddr,
    pub len: u64,
}

#[derive(Debug, Default)]
struct SysvShmState {
    next_shmid: i32,
    segments: BTreeMap<i32, SysvShmSegment>,
}

#[derive(Clone, Debug)]
struct SysvShmSegment {
    key: i32,
    size: usize,
    frames: Arc<[PhysFrame<Size4KiB>]>,
    owner_uid: u32,
    owner_gid: u32,
    creator_uid: u32,
    creator_gid: u32,
    mode: u32,
    seq: i32,
    creator_pid: i32,
    last_pid: i32,
    attach_count: u64,
    atime: i64,
    dtime: i64,
    ctime: i64,
    marked_for_removal: bool,
}

impl SysvShmState {
    fn next_shmid(&mut self) -> i32 {
        self.next_shmid += 1;
        self.next_shmid
    }
}

fn now_seconds() -> i64 {
    unix_timestamp_seconds().min(i64::MAX as u64) as i64
}

fn mapping_pages(len: u64) -> u64 {
    len.div_ceil(PAGE_SIZE)
}

fn allocate_segment_frames(pages: u64) -> Result<Arc<[PhysFrame<Size4KiB>]>, SyscallError> {
    let mut frames = Vec::with_capacity(pages as usize);
    let mut allocator = FRAME_ALLOCATOR.get().unwrap().lock();

    for _ in 0..pages {
        let frame = allocator.allocate_frame().ok_or(SyscallError::NoMemory)?;
        unsafe {
            core::ptr::write_bytes(
                apply_offset(frame.start_address().as_u64()) as *mut u8,
                0,
                4096,
            );
        }
        increase_ref(frame);
        frames.push(frame);
    }

    Ok(frames.into())
}

fn destroy_segment_locked(state: &mut SysvShmState, shmid: i32) {
    let Some(segment) = state.segments.remove(&shmid) else {
        return;
    };

    let mut allocator = FRAME_ALLOCATOR.get().unwrap().lock();
    for frame in segment.frames.iter().copied() {
        if decrease_ref(frame) {
            unsafe {
                allocator.deallocate_frame(frame);
            }
        }
    }
}

fn segment_allows_access(segment: &SysvShmSegment, process: &Process, readonly: bool) -> bool {
    if process.effective_uid == 0 {
        return true;
    }

    let mut mask = 0o4u32;
    if !readonly {
        mask |= 0o2;
    }

    let mode = segment.mode;
    if process.effective_uid == segment.owner_uid || process.effective_uid == segment.creator_uid {
        return mode & (mask << 6) == (mask << 6);
    }
    if process.effective_gid == segment.owner_gid
        || process.effective_gid == segment.creator_gid
        || process
            .supplementary_groups
            .iter()
            .any(|gid| *gid == segment.owner_gid || *gid == segment.creator_gid)
    {
        return mode & (mask << 3) == (mask << 3);
    }

    mode & mask == mask
}

fn attach_addr(
    requested: VirtAddr,
    flags: i32,
    process: &mut Process,
    len: u64,
) -> SyscallResult<VirtAddr> {
    if requested.is_null() {
        return Ok(process.addrspace.fetch_add_user_mem(mapping_pages(len)));
    }

    if flags & SHM_RND != 0 {
        return Ok(VirtAddr::new(requested.as_u64() & !(SHMLBA - 1)));
    }

    if !requested.as_u64().is_multiple_of(PAGE_SIZE) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(requested)
}

fn complete_detach(process: &mut Process, mapping: ProcessShmMapping) {
    process.addrspace.unmap(mapping.addr, mapping.len);

    let mut state = SYSV_SHM_STATE.lock();
    let Some(segment) = state.segments.get_mut(&mapping.shmid) else {
        return;
    };
    segment.attach_count = segment.attach_count.saturating_sub(1);
    segment.last_pid = process.pid.0 as i32;
    segment.dtime = now_seconds();
    let should_destroy = segment.marked_for_removal && segment.attach_count == 0;
    if should_destroy {
        destroy_segment_locked(&mut state, mapping.shmid);
    }
}

pub fn inherit_forked_mappings(mappings: &[ProcessShmMapping]) {
    let mut state = SYSV_SHM_STATE.lock();
    for mapping in mappings {
        if let Some(segment) = state.segments.get_mut(&mapping.shmid) {
            segment.attach_count = segment.attach_count.saturating_add(1);
        }
    }
}

pub fn detach_all_process_mappings(process: &mut Process) {
    let mappings = mem::take(&mut process.sysv_shm_mappings);
    for mapping in mappings {
        complete_detach(process, mapping);
    }
}

pub fn shmget(process: &Process, key: i32, size: usize, shmflg: i32) -> SyscallResult {
    let create = shmflg & IPC_CREAT != 0;
    let exclusive = shmflg & IPC_EXCL != 0;
    let mode = (shmflg & IPC_MODE_MASK) as u32;

    let mut state = SYSV_SHM_STATE.lock();
    if key != IPC_PRIVATE
        && let Some((existing_id, existing)) = state
            .segments
            .iter()
            .find(|(_, segment)| segment.key == key && !segment.marked_for_removal)
    {
        if create && exclusive {
            return Err(SyscallError::FileAlreadyExists);
        }
        if size != 0 && size > existing.size {
            return Err(SyscallError::InvalidArguments);
        }
        return Ok(*existing_id as usize);
    }

    if !create && key != IPC_PRIVATE {
        return Err(SyscallError::FileNotFound);
    }
    if size == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let pages = (size as u64).div_ceil(PAGE_SIZE);
    let frames = allocate_segment_frames(pages)?;
    let shmid = state.next_shmid();
    let now = now_seconds();
    state.segments.insert(
        shmid,
        SysvShmSegment {
            key,
            size,
            frames,
            owner_uid: process.effective_uid,
            owner_gid: process.effective_gid,
            creator_uid: process.effective_uid,
            creator_gid: process.effective_gid,
            mode,
            seq: 0,
            creator_pid: process.pid.0 as i32,
            last_pid: process.pid.0 as i32,
            attach_count: 0,
            atime: 0,
            dtime: 0,
            ctime: now,
            marked_for_removal: false,
        },
    );
    Ok(shmid as usize)
}

pub fn shmat(process: &mut Process, shmid: i32, shmaddr: *const u8, shmflg: i32) -> SyscallResult {
    if shmflg & !(SHM_RDONLY | SHM_RND) != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let (frames, len) = {
        let state = SYSV_SHM_STATE.lock();
        let segment = state
            .segments
            .get(&shmid)
            .ok_or(SyscallError::InvalidArguments)?;
        let readonly = shmflg & SHM_RDONLY != 0;
        if !segment_allows_access(segment, process, readonly) {
            return Err(SyscallError::PermissionDenied);
        }
        (segment.frames.clone(), segment.size as u64)
    };

    let addr = attach_addr(VirtAddr::new(shmaddr as u64), shmflg, process, len)?;
    let pages = mapping_pages(len);
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | if shmflg & SHM_RDONLY == 0 {
            PageTableFlags::WRITABLE
        } else {
            PageTableFlags::empty()
        };

    for frame in frames.iter().copied() {
        increase_ref(frame);
    }

    process.addrspace.map(MemoryArea::new(
        addr,
        pages,
        flags,
        Data::Shared {
            frames,
            flags: PageTableFlags::empty(),
        },
        false,
    ));
    process.sysv_shm_mappings.push(ProcessShmMapping {
        shmid,
        addr,
        len: pages * PAGE_SIZE,
    });

    let mut state = SYSV_SHM_STATE.lock();
    let segment = state
        .segments
        .get_mut(&shmid)
        .ok_or(SyscallError::InvalidArguments)?;
    segment.attach_count = segment.attach_count.saturating_add(1);
    segment.last_pid = process.pid.0 as i32;
    segment.atime = now_seconds();

    Ok(addr.as_u64() as usize)
}

pub fn shmdt(process: &mut Process, shmaddr: *const u8) -> SyscallResult {
    let addr = VirtAddr::new(shmaddr as u64);
    let Some(index) = process
        .sysv_shm_mappings
        .iter()
        .position(|mapping| mapping.addr == addr)
    else {
        return Err(SyscallError::InvalidArguments);
    };

    let mapping = process.sysv_shm_mappings.remove(index);
    complete_detach(process, mapping);
    Ok(0)
}

pub fn shmctl(process: &Process, shmid: i32, cmd: i32, buf: *mut LinuxShmidDs) -> SyscallResult {
    let mut state = SYSV_SHM_STATE.lock();
    match cmd {
        IPC_RMID => {
            let segment = state
                .segments
                .get_mut(&shmid)
                .ok_or(SyscallError::InvalidArguments)?;
            if process.effective_uid != 0
                && process.effective_uid != segment.owner_uid
                && process.effective_uid != segment.creator_uid
            {
                return Err(SyscallError::PermissionDenied);
            }
            segment.marked_for_removal = true;
            segment.ctime = now_seconds();
            if segment.attach_count == 0 {
                destroy_segment_locked(&mut state, shmid);
            }
            Ok(0)
        }
        IPC_STAT => {
            if buf.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let segment = state
                .segments
                .get(&shmid)
                .ok_or(SyscallError::InvalidArguments)?;
            let ds = LinuxShmidDs {
                shm_perm: LinuxIpcPerm {
                    __ipc_perm_key: segment.key,
                    uid: segment.owner_uid,
                    gid: segment.owner_gid,
                    cuid: segment.creator_uid,
                    cgid: segment.creator_gid,
                    mode: segment.mode,
                    __ipc_perm_seq: segment.seq,
                    __pad1: 0,
                    __pad2: 0,
                },
                shm_segsz: segment.size,
                shm_atime: segment.atime,
                shm_dtime: segment.dtime,
                shm_ctime: segment.ctime,
                shm_cpid: segment.creator_pid,
                shm_lpid: segment.last_pid,
                shm_nattch: segment.attach_count,
                __pad1: 0,
                __pad2: 0,
            };
            drop(state);
            crate::memory::user_safe::write(buf, &ds)?;
            Ok(0)
        }
        _ => Err(SyscallError::InvalidArguments),
    }
}
