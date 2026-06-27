use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc};

#[derive(Debug, Default)]
pub struct TimeNamespace {
    monotonic_offset_ns: Mut<i64>,
    boottime_offset_ns: Mut<i64>,
}

pub type TimeNamespaceRef = Arc<TimeNamespace>;

impl TimeNamespace {
    pub fn new() -> TimeNamespaceRef {
        Arc::new(Self::default())
    }

    pub fn monotonic_offset_ns(&self) -> i64 {
        *self.monotonic_offset_ns.lock()
    }

    pub fn boottime_offset_ns(&self) -> i64 {
        *self.boottime_offset_ns.lock()
    }

    pub fn set_offsets(&self, monotonic_offset_ns: i64, boottime_offset_ns: i64) {
        *self.monotonic_offset_ns.lock() = monotonic_offset_ns;
        *self.boottime_offset_ns.lock() = boottime_offset_ns;
    }

    pub fn offsets_text(&self) -> String {
        let (monotonic_sec, monotonic_nsec) = split_ns(self.monotonic_offset_ns());
        let (boottime_sec, boottime_nsec) = split_ns(self.boottime_offset_ns());
        alloc::format!(
            "monotonic {monotonic_sec} {monotonic_nsec}\nboottime {boottime_sec} {boottime_nsec}\n"
        )
    }
}

fn split_ns(ns: i64) -> (i64, i64) {
    let sec = ns.div_euclid(1_000_000_000);
    let nsec = ns.rem_euclid(1_000_000_000);
    (sec, nsec)
}
