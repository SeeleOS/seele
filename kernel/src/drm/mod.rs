mod allocation;
mod buffer_handlers;
pub mod card;
pub mod client;
mod client_handlers;
mod configure;
mod cursor_handlers;
mod display_handlers;
mod events;
mod framebuffer;
pub mod fs;
pub mod mode;
pub mod mode_types;
pub mod object;
pub(crate) mod prime;
mod state;
mod user;

#[cfg(test)]
mod test;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::misc::time::{NANOSECONDS_PER_MILLISECOND, Time};

const SCANOUT_REFRESH_INTERVAL_NS: u64 = 100 * NANOSECONDS_PER_MILLISECOND;
static NEXT_SCANOUT_REFRESH_NS: AtomicU64 = AtomicU64::new(0);

pub fn poll_scanout_refresh(now: Time) {
    let now_ns = now.as_nanoseconds();
    let next_ns = NEXT_SCANOUT_REFRESH_NS.load(Ordering::Relaxed);
    if now_ns < next_ns {
        return;
    }

    if NEXT_SCANOUT_REFRESH_NS
        .compare_exchange(
            next_ns,
            now_ns.saturating_add(SCANOUT_REFRESH_INTERVAL_NS),
            Ordering::AcqRel,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        let _ = framebuffer::refresh_current_scanout();
    }
}
