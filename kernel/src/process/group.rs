use core::sync::atomic::AtomicU64;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessGroupID(pub u64);

impl Default for ProcessGroupID {
    fn default() -> Self {
        static NEXT_GID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_GID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}
