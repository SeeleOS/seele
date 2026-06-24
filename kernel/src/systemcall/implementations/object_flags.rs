use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct FallocateFlags: i32 {
        const FALLOC_FL_KEEP_SIZE = 0x01;
        const FALLOC_FL_PUNCH_HOLE = 0x02;
        const FALLOC_FL_COLLAPSE_RANGE = 0x08;
        const FALLOC_FL_ZERO_RANGE = 0x10;
        const FALLOC_FL_INSERT_RANGE = 0x20;
        const FALLOC_FL_UNSHARE_RANGE = 0x40;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct DupFlags: i32 {
        const O_CLOEXEC = 0o2_000_000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct CloseRangeFlags: u32 {
        const CLOSE_RANGE_UNSHARE = 0x2;
        const CLOSE_RANGE_CLOEXEC = 0x4;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct MemfdFlags: u32 {
        const MFD_CLOEXEC = 0x0001;
        const MFD_ALLOW_SEALING = 0x0002;
        const MFD_NOEXEC_SEAL = 0x0008;
        const MFD_EXEC = 0x0010;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct PositionedIoFlags: i32 {
        const RWF_HIPRI = 0x00000001;
        const RWF_DSYNC = 0x00000002;
        const RWF_SYNC = 0x00000004;
        const RWF_NOWAIT = 0x00000008;
        const RWF_APPEND = 0x00000010;
    }
}
