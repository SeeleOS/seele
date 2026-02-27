use alloc::sync::Arc;
use spin::Mutex;

use crate::multitasking::process::process::Process;

pub mod manager;
pub mod misc;
pub mod process;

pub type ProcessRef = Arc<Mutex<Process>>;
