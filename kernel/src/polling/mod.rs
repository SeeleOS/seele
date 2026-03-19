//! Polling is the kernel-side model behind Linux-style `epoll`.
//!
//! The important idea is that a thread does not block on a global event type
//! like "keypress". It blocks on one specific poller object.
//!
//! A poller object owns a set of registrations:
//! - which object is being watched
//! - which event on that object is interesting
//!
//! The flow is:
//! 1. userspace creates a poller (`epoll_create1` -> `CreatePoller`)
//! 2. userspace registers watched objects (`epoll_ctl` -> add/remove on poller)
//! 3. userspace waits on the poller (`epoll_wait`)
//!
//! When waiting, the current thread should be blocked with a reason like
//! `BlockedType::Poller(...)`. The blocked reason needs to identify the poller
//! itself, not just a bare event kind, because different pollers may watch
//! different objects for the same event.
//!
//! When an object becomes ready, the wake path should:
//! 1. determine which poller(s) watch that object/event pair
//! 2. wake the threads blocked on those pollers
//! 3. let the resumed wait path rescan the poller entries and report ready
//!    events back to userspace
//!
//! In other words:
//! - `event.rs` describes event kinds produced by one specific object
//! - `poller.rs` stores the watched `(object, event)` registrations
//! - `wake.rs` is responsible for waking waiters when a watched object changes

pub mod event;
pub mod poller;
pub mod wake;
