use super::*;

pub(super) struct CgroupFileHandle {
    path: String,
    kind: CgroupFileKind,
    offset: usize,
}

impl CgroupFileHandle {
    pub(super) fn new(path: String, kind: CgroupFileKind) -> Self {
        Self {
            path,
            kind,
            offset: 0,
        }
    }
}

impl File for CgroupFileHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        file_info(&self.path, self.kind)
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let state = CGROUP_STATE.lock();
        let data = file_contents(&state, &self.path, self.kind)?;
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }

        let len = buffer.len().min(data.len() - offset);
        buffer[..len].copy_from_slice(&data[offset..offset + len]);
        Ok(len)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = self.read_at(buffer, self.offset as u64)?;
        self.offset += read;
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let written = write_file(&self.path, self.kind, buffer)?;
        self.offset += written;
        Ok(written)
    }

    fn truncate(&mut self, _length: u64) -> FSResult<()> {
        match self.kind {
            CgroupFileKind::Controllers
            | CgroupFileKind::Events
            | CgroupFileKind::Type
            | CgroupFileKind::CpuStat
            | CgroupFileKind::MemoryCurrent => Err(FSError::Readonly),
            CgroupFileKind::Procs
            | CgroupFileKind::Threads
            | CgroupFileKind::SubtreeControl
            | CgroupFileKind::Kill
            | CgroupFileKind::Freeze
            | CgroupFileKind::CpuMax
            | CgroupFileKind::MemoryMin
            | CgroupFileKind::MemoryLow
            | CgroupFileKind::MemoryHigh
            | CgroupFileKind::MemoryMax
            | CgroupFileKind::MemorySwapMax
            | CgroupFileKind::MemoryOomGroup
            | CgroupFileKind::MemoryReclaim
            | CgroupFileKind::PidsMax => Ok(()),
        }
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = file_contents(&CGROUP_STATE.lock(), &self.path, self.kind)?.len() as i64;
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.offset as i64 + offset,
            Whence::End => len + offset,
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(FSError::Other);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(FSError::Other);
                }
                len
            }
        };
        if next < 0 {
            return Err(FSError::InvalidArguments);
        }
        self.offset = next as usize;
        Ok(self.offset)
    }

    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Ok(())
    }
}
