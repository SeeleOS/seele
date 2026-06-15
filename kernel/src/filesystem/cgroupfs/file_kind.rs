use super::*;

#[derive(Clone, Copy)]
pub(super) enum CgroupFileKind {
    Procs,
    Threads,
    Controllers,
    SubtreeControl,
    Events,
    Kill,
    Freeze,
    Type,
    CpuMax,
    CpuStat,
    MemoryCurrent,
    MemoryMin,
    MemoryLow,
    MemoryHigh,
    MemoryMax,
    MemorySwapMax,
    MemoryOomGroup,
    MemoryReclaim,
    PidsMax,
}

impl CgroupFileKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Procs => "cgroup.procs",
            Self::Threads => "cgroup.threads",
            Self::Controllers => "cgroup.controllers",
            Self::SubtreeControl => "cgroup.subtree_control",
            Self::Events => "cgroup.events",
            Self::Kill => "cgroup.kill",
            Self::Freeze => "cgroup.freeze",
            Self::Type => "cgroup.type",
            Self::CpuMax => "cpu.max",
            Self::CpuStat => "cpu.stat",
            Self::MemoryCurrent => "memory.current",
            Self::MemoryMin => "memory.min",
            Self::MemoryLow => "memory.low",
            Self::MemoryHigh => "memory.high",
            Self::MemoryMax => "memory.max",
            Self::MemorySwapMax => "memory.swap.max",
            Self::MemoryOomGroup => "memory.oom.group",
            Self::MemoryReclaim => "memory.reclaim",
            Self::PidsMax => "pids.max",
        }
    }

    pub(super) fn inode_offset(self) -> u64 {
        match self {
            Self::Procs => 1,
            Self::Threads => 2,
            Self::Controllers => 3,
            Self::SubtreeControl => 4,
            Self::Events => 5,
            Self::Kill => 6,
            Self::Freeze => 7,
            Self::Type => 8,
            Self::CpuMax => 9,
            Self::CpuStat => 10,
            Self::MemoryCurrent => 11,
            Self::MemoryMin => 12,
            Self::MemoryLow => 13,
            Self::MemoryHigh => 14,
            Self::MemoryMax => 15,
            Self::MemorySwapMax => 16,
            Self::MemoryOomGroup => 17,
            Self::MemoryReclaim => 18,
            Self::PidsMax => 19,
        }
    }

    pub(super) fn mode(self) -> u32 {
        match self {
            Self::Controllers | Self::Events | Self::Type | Self::CpuStat | Self::MemoryCurrent => {
                READONLY_FILE_MODE
            }
            Self::Procs
            | Self::Threads
            | Self::SubtreeControl
            | Self::Kill
            | Self::Freeze
            | Self::CpuMax
            | Self::MemoryMin
            | Self::MemoryLow
            | Self::MemoryHigh
            | Self::MemoryMax
            | Self::MemorySwapMax
            | Self::MemoryOomGroup
            | Self::MemoryReclaim
            | Self::PidsMax => WRITABLE_FILE_MODE,
        }
    }

    pub(super) fn all() -> &'static [Self] {
        &[
            Self::Procs,
            Self::Threads,
            Self::Controllers,
            Self::SubtreeControl,
            Self::Events,
            Self::Kill,
            Self::Freeze,
            Self::Type,
            Self::CpuMax,
            Self::CpuStat,
            Self::MemoryCurrent,
            Self::MemoryMin,
            Self::MemoryLow,
            Self::MemoryHigh,
            Self::MemoryMax,
            Self::MemorySwapMax,
            Self::MemoryOomGroup,
            Self::MemoryReclaim,
            Self::PidsMax,
        ]
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|kind| kind.name() == name)
    }
}
