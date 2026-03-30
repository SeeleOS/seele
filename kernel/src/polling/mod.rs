//! This module implements the kernel-side "wait for many objects at once"
//! system used to model Linux-style `epoll`.
//!
//! a thread like `bash` does not want to busy-loop asking whether each object
//! is ready. Instead, it wants to say:
//! - here is a set of objects I care about
//! - wake me when any of them becomes ready
//!
//! The kernel represents that request with a poller object.
//!
//! A poller is not itself "the event". It is a container describing interest
//! in multiple object-specific events:
//! - object A readable
//! - object B writable
//! - object C hung up
//!
//! Userspace interaction is then:
//! 1. create one poller
//! 2. add or remove watched `(object, event)` pairs on that poller
//! 3. wait on that poller
//!
//! The key design point is that a waiting thread should block on one specific
//! poller, not on a global event kind like "keypress". Different pollers may
//! watch different objects for the same event type, so the blocked reason must
//! identify the poller itself.
//!
//! When a watched object changes state, the wake path should notify the
//! relevant poller(s). Threads blocked on those pollers can then resume and
//! rescan the poller state to learn which watched objects are actually ready.
//!
//! Module split:
//! - `event.rs`: event kinds that one specific object can report
//! - `poller.rs`: the poller object and its watched registrations
//! - `wake.rs`: logic that wakes threads waiting on affected pollers

mod entry;
pub mod event;
pub mod object;
pub mod poller;
mod ready;
mod registration;
mod wake;

pub use entry::PollerEntry;
pub use object::PollerObject;
pub use ready::PollerReadyEvent;
