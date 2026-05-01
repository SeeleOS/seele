use alloc::{
    collections::{BTreeMap, vec_deque::VecDeque},
    string::String,
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        info::DirectoryContentInfo,
        vfs_traits::{DirectoryContentType, FileLikeType},
    },
    object::{FileFlags, error::ObjectError, queue_helpers::read_or_block_with_flags},
    process::manager::get_current_process,
    thread::{
        THREAD_MANAGER, get_current_thread,
        yielding::{
            BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
        },
    },
};

use super::protocol::{
    FATTR_GID, FATTR_MODE, FATTR_SIZE, FATTR_UID, FUSE_ASYNC_READ, FUSE_ATOMIC_O_TRUNC,
    FUSE_BIG_WRITES, FUSE_GETATTR, FUSE_INIT, FUSE_KERNEL_MINOR_VERSION, FUSE_KERNEL_VERSION,
    FUSE_LOOKUP, FUSE_MIN_READ_BUFFER, FUSE_OPEN, FUSE_OPENDIR, FUSE_PARALLEL_DIROPS, FUSE_READ,
    FUSE_READDIR, FUSE_READLINK, FUSE_RELEASE, FUSE_RELEASEDIR, FUSE_ROOT_ID, FUSE_SETATTR,
    FUSE_WRITE, FuseAttr, FuseAttrOut, FuseDirentHeader, FuseEntryOut, FuseGetattrIn, FuseInHeader,
    FuseInitIn, FuseInitOut, FuseOpenIn, FuseOpenOut, FuseOutHeader, FuseReadIn, FuseReleaseIn,
    FuseSetattrIn, FuseWriteIn, FuseWriteOut, RequestContext, as_bytes, dirent_record_len,
    read_pod,
};

const READDIR_CHUNK: u32 = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct FuseOpenedHandle {
    pub fh: u64,
    pub open_flags: u32,
}

#[derive(Clone, Debug)]
pub struct FuseLookupEntry {
    pub nodeid: u64,
    pub attr: FuseAttr,
}

#[derive(Clone, Debug)]
pub struct FuseDirEntry {
    pub info: DirectoryContentInfo,
    pub offset: u64,
}

#[derive(Debug)]
enum ResponseRecord {
    Pending,
    Ready(Result<Vec<u8>, i32>),
}

#[derive(Debug)]
struct FuseConnectionState {
    mounted: bool,
    init_unique: Option<u64>,
    init_complete: bool,
    next_unique: u64,
    pending_requests: VecDeque<Vec<u8>>,
    responses: BTreeMap<u64, ResponseRecord>,
}

#[derive(Debug)]
pub struct FuseConnection {
    state: Mutex<FuseConnectionState>,
}

impl FuseConnection {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FuseConnectionState {
                mounted: false,
                init_unique: None,
                init_complete: false,
                next_unique: 1,
                pending_requests: VecDeque::new(),
                responses: BTreeMap::new(),
            }),
        })
    }

    pub fn daemon_read(&self, buffer: &mut [u8], flags: FileFlags) -> Result<usize, ObjectError> {
        if buffer.len() < FUSE_MIN_READ_BUFFER {
            return Err(ObjectError::InvalidArguments);
        }

        read_or_block_with_flags(buffer, flags, WakeType::IO, |buffer| {
            let mut state = self.state.lock();
            let request = state.pending_requests.pop_front()?;
            let len = request.len().min(buffer.len());
            buffer[..len].copy_from_slice(&request[..len]);
            Some(len)
        })
    }

    pub fn daemon_write(&self, buffer: &[u8]) -> Result<usize, ObjectError> {
        let mut consumed = 0usize;
        while consumed < buffer.len() {
            let remaining = &buffer[consumed..];
            let header =
                read_pod::<FuseOutHeader>(remaining).ok_or(ObjectError::InvalidArguments)?;
            let total_len = header.len as usize;
            if total_len < core::mem::size_of::<FuseOutHeader>() || total_len > remaining.len() {
                return Err(ObjectError::InvalidArguments);
            }

            let payload = remaining[core::mem::size_of::<FuseOutHeader>()..total_len].to_vec();
            let response = if header.error == 0 {
                Ok(payload)
            } else {
                Err((-header.error).max(0))
            };

            let mut state = self.state.lock();
            let Some(record) = state.responses.get_mut(&header.unique) else {
                return Err(ObjectError::InvalidArguments);
            };
            *record = ResponseRecord::Ready(response);
            if state.init_unique == Some(header.unique) {
                state.init_unique = None;
                state.init_complete = true;
            }
            drop(state);

            THREAD_MANAGER.get().unwrap().lock().wake_io();
            consumed += total_len;
        }

        Ok(buffer.len())
    }

    pub fn is_request_pending(&self) -> bool {
        !self.state.lock().pending_requests.is_empty()
    }

    pub fn mount_ready(&self) -> Result<(), FSError> {
        let mut state = self.state.lock();
        if state.mounted {
            return Err(FSError::Busy);
        }
        state.mounted = true;
        if state.init_unique.is_none() && !state.init_complete {
            let unique = state.next_unique;
            state.next_unique += 1;
            state.responses.insert(unique, ResponseRecord::Pending);
            state.init_unique = Some(unique);
            let request = build_init_request(unique);
            state.pending_requests.push_back(request);
        }
        drop(state);

        THREAD_MANAGER.get().unwrap().lock().wake_io();
        Ok(())
    }

    pub fn ensure_ready(&self) -> Result<(), FSError> {
        let init_unique = {
            let state = self.state.lock();
            if state.init_complete {
                return Ok(());
            }
            state.init_unique.ok_or(FSError::Other)?
        };

        let payload = self.wait_for_response(init_unique)?;
        let init = read_pod::<FuseInitOut>(&payload).ok_or(FSError::Other)?;
        if init.major != FUSE_KERNEL_VERSION {
            return Err(FSError::Other);
        }
        Ok(())
    }

    pub fn lookup(&self, parent: u64, name: &str) -> Result<FuseLookupEntry, FSError> {
        let mut payload = name.as_bytes().to_vec();
        payload.push(0);
        let response = self.request(FUSE_LOOKUP, parent, payload)?;
        let entry = read_pod::<FuseEntryOut>(&response).ok_or(FSError::Other)?;
        if entry.nodeid == 0 {
            return Err(FSError::NotFound);
        }
        Ok(FuseLookupEntry {
            nodeid: entry.nodeid,
            attr: entry.attr,
        })
    }

    pub fn getattr(&self, nodeid: u64) -> Result<FuseAttr, FSError> {
        let input = FuseGetattrIn::default();
        let response = self.request(FUSE_GETATTR, nodeid, as_bytes(&input).to_vec())?;
        let attr = read_pod::<FuseAttrOut>(&response).ok_or(FSError::Other)?;
        Ok(attr.attr)
    }

    pub fn readlink(&self, nodeid: u64) -> Result<String, FSError> {
        let response = self.request(FUSE_READLINK, nodeid, Vec::new())?;
        let end = response
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(response.len());
        String::from_utf8(response[..end].to_vec()).map_err(|_| FSError::Other)
    }

    pub fn open_dir(&self, nodeid: u64) -> Result<FuseOpenedHandle, FSError> {
        self.open(nodeid, FUSE_OPENDIR)
    }

    pub fn read_dir(&self, nodeid: u64) -> Result<Vec<FuseDirEntry>, FSError> {
        let handle = self.open_dir(nodeid)?;
        let mut entries = Vec::new();
        let mut offset = 0u64;

        loop {
            let input = FuseReadIn {
                fh: handle.fh,
                offset,
                size: READDIR_CHUNK,
                ..Default::default()
            };
            let response = self.request(FUSE_READDIR, nodeid, as_bytes(&input).to_vec())?;
            if response.is_empty() {
                break;
            }

            let mut cursor = 0usize;
            while cursor < response.len() {
                let record = &response[cursor..];
                let header = read_pod::<FuseDirentHeader>(record).ok_or(FSError::Other)?;
                let header_len = core::mem::size_of::<FuseDirentHeader>();
                let name_start = cursor + header_len;
                let name_end = name_start + header.namelen as usize;
                if name_end > response.len() {
                    return Err(FSError::Other);
                }

                let name = String::from_utf8(response[name_start..name_end].to_vec())
                    .map_err(|_| FSError::Other)?;
                entries.push(FuseDirEntry {
                    info: DirectoryContentInfo::new(name, dirent_type(header.type_))
                        .with_inode(header.ino),
                    offset: header.off,
                });
                offset = header.off;
                cursor += dirent_record_len(header.namelen as usize);
            }
        }

        let _ = self.release(nodeid, handle, FUSE_RELEASEDIR);
        Ok(entries)
    }

    pub fn open_file(&self, nodeid: u64, flags: u32) -> Result<FuseOpenedHandle, FSError> {
        self.open_with_flags(nodeid, FUSE_OPEN, flags)
    }

    pub fn read_file(&self, nodeid: u64, offset: u64, size: u32) -> Result<Vec<u8>, FSError> {
        let handle = self.open_file(nodeid, 0)?;
        let input = FuseReadIn {
            fh: handle.fh,
            offset,
            size,
            ..Default::default()
        };
        let response = self.request(FUSE_READ, nodeid, as_bytes(&input).to_vec())?;
        let _ = self.release(nodeid, handle, FUSE_RELEASE);
        Ok(response)
    }

    pub fn write_file(&self, nodeid: u64, offset: u64, buffer: &[u8]) -> Result<usize, FSError> {
        let handle = self.open_file(nodeid, 1)?;
        let mut payload = as_bytes(&FuseWriteIn {
            fh: handle.fh,
            offset,
            size: buffer.len() as u32,
            ..Default::default()
        })
        .to_vec();
        payload.extend_from_slice(buffer);
        let response = self.request(FUSE_WRITE, nodeid, payload)?;
        let written = read_pod::<FuseWriteOut>(&response)
            .ok_or(FSError::Other)?
            .size as usize;
        let _ = self.release(nodeid, handle, FUSE_RELEASE);
        Ok(written)
    }

    pub fn setattr_size(&self, nodeid: u64, size: u64) -> Result<FuseAttr, FSError> {
        let input = FuseSetattrIn {
            valid: FATTR_SIZE,
            size,
            ..Default::default()
        };
        let response = self.request(FUSE_SETATTR, nodeid, as_bytes(&input).to_vec())?;
        Ok(read_pod::<FuseAttrOut>(&response)
            .ok_or(FSError::Other)?
            .attr)
    }

    pub fn setattr_mode(
        &self,
        nodeid: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<FuseAttr, FSError> {
        let mut valid = 0u32;
        let mut input = FuseSetattrIn::default();
        if let Some(mode) = mode {
            valid |= FATTR_MODE;
            input.mode = mode;
        }
        if let Some(uid) = uid {
            valid |= FATTR_UID;
            input.uid = uid;
        }
        if let Some(gid) = gid {
            valid |= FATTR_GID;
            input.gid = gid;
        }
        input.valid = valid;
        let response = self.request(FUSE_SETATTR, nodeid, as_bytes(&input).to_vec())?;
        Ok(read_pod::<FuseAttrOut>(&response)
            .ok_or(FSError::Other)?
            .attr)
    }

    pub fn root_id(&self) -> u64 {
        FUSE_ROOT_ID
    }

    fn open(&self, nodeid: u64, opcode: u32) -> Result<FuseOpenedHandle, FSError> {
        self.open_with_flags(nodeid, opcode, 0)
    }

    fn open_with_flags(
        &self,
        nodeid: u64,
        opcode: u32,
        flags: u32,
    ) -> Result<FuseOpenedHandle, FSError> {
        let input = FuseOpenIn {
            flags,
            open_flags: 0,
        };
        let response = self.request(opcode, nodeid, as_bytes(&input).to_vec())?;
        let open = read_pod::<FuseOpenOut>(&response).ok_or(FSError::Other)?;
        Ok(FuseOpenedHandle {
            fh: open.fh,
            open_flags: open.open_flags,
        })
    }

    fn release(&self, nodeid: u64, handle: FuseOpenedHandle, opcode: u32) -> Result<(), FSError> {
        let input = FuseReleaseIn {
            fh: handle.fh,
            ..Default::default()
        };
        let _ = self.request(opcode, nodeid, as_bytes(&input).to_vec())?;
        Ok(())
    }

    fn request(&self, opcode: u32, nodeid: u64, payload: Vec<u8>) -> Result<Vec<u8>, FSError> {
        self.ensure_ready()?;

        let unique = {
            let mut state = self.state.lock();
            let unique = state.next_unique;
            state.next_unique += 1;
            state.responses.insert(unique, ResponseRecord::Pending);
            let ctx = current_request_context();
            state
                .pending_requests
                .push_back(build_request(opcode, unique, nodeid, ctx, &payload));
            unique
        };

        THREAD_MANAGER.get().unwrap().lock().wake_io();
        self.wait_for_response(unique)
    }

    fn wait_for_response(&self, unique: u64) -> Result<Vec<u8>, FSError> {
        loop {
            if let Some(result) = {
                let mut state = self.state.lock();
                match state.responses.get(&unique) {
                    Some(ResponseRecord::Ready(_)) => {
                        let Some(ResponseRecord::Ready(result)) = state.responses.remove(&unique)
                        else {
                            unreachable!();
                        };
                        Some(result)
                    }
                    Some(ResponseRecord::Pending) => None,
                    None => Some(Err(5)),
                }
            } {
                return result.map_err(errno_to_fs_error);
            }

            if !get_current_process().lock().pending_signals.is_empty() {
                return Err(FSError::Other);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            let ready = {
                let state = self.state.lock();
                matches!(state.responses.get(&unique), Some(ResponseRecord::Ready(_)))
            };

            if ready {
                cancel_block(&current);
            } else {
                finish_block_current();
            }
        }
    }
}

fn build_init_request(unique: u64) -> Vec<u8> {
    let input = FuseInitIn {
        major: FUSE_KERNEL_VERSION,
        minor: FUSE_KERNEL_MINOR_VERSION,
        max_readahead: FUSE_MIN_READ_BUFFER as u32,
        flags: FUSE_ASYNC_READ | FUSE_ATOMIC_O_TRUNC | FUSE_BIG_WRITES | FUSE_PARALLEL_DIROPS,
        ..Default::default()
    };
    build_request(
        FUSE_INIT,
        unique,
        0,
        RequestContext {
            uid: 0,
            gid: 0,
            pid: 0,
        },
        as_bytes(&input),
    )
}

fn build_request(
    opcode: u32,
    unique: u64,
    nodeid: u64,
    ctx: RequestContext,
    payload: &[u8],
) -> Vec<u8> {
    let header = FuseInHeader {
        len: (core::mem::size_of::<FuseInHeader>() + payload.len()) as u32,
        opcode,
        unique,
        nodeid,
        uid: ctx.uid,
        gid: ctx.gid,
        pid: ctx.pid,
        total_extlen: 0,
        padding: 0,
    };

    let mut request = Vec::with_capacity(header.len as usize);
    request.extend_from_slice(as_bytes(&header));
    request.extend_from_slice(payload);
    request
}

fn current_request_context() -> RequestContext {
    let (uid, gid, pid) = {
        let process = get_current_process();
        let process = process.lock();
        (process.fs_uid, process.fs_gid, process.pid.0 as u32)
    };
    let _tid = get_current_thread().lock().id.0;
    RequestContext { uid, gid, pid }
}

fn dirent_type(kind: u32) -> DirectoryContentType {
    match kind {
        4 => DirectoryContentType::Directory,
        10 => DirectoryContentType::Symlink,
        _ => DirectoryContentType::File,
    }
}

pub fn attr_file_type(attr: FuseAttr) -> FileLikeType {
    match attr.mode & 0o170000 {
        0o040000 => FileLikeType::Directory,
        0o120000 => FileLikeType::Symlink,
        _ => FileLikeType::File,
    }
}

fn errno_to_fs_error(errno: i32) -> FSError {
    match errno {
        2 => FSError::NotFound,
        13 => FSError::AccessDenied,
        17 => FSError::AlreadyExists,
        20 => FSError::NotADirectory,
        21 => FSError::NotAFile,
        28 => FSError::NoSpace,
        39 => FSError::DirectoryNotEmpty,
        _ => FSError::Other,
    }
}
