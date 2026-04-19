use core::sync::atomic::{AtomicBool, Ordering};

static CTRL_ALT_DEL_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_ctrl_alt_del_enabled(enabled: bool) {
    CTRL_ALT_DEL_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn ctrl_alt_del_enabled() -> bool {
    CTRL_ALT_DEL_ENABLED.load(Ordering::Relaxed)
}
