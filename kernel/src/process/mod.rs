use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use seele_sys::signal::Signals;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::filesystem::absolute_path::AbsolutePath;
use crate::memory::addrspace::AddrSpace;
use crate::signal::action::SignalAction;
use crate::signal::misc::default_signal_action_vec;
use crate::{object::Object, process::misc::ProcessID, thread::thread::Thread};

pub mod execve;
pub mod fork;
pub mod manager;
pub mod misc;
pub mod new;
pub mod object;

pub type ProcessRef = Arc<Mutex<Process>>;

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub addrspace: AddrSpace,
    pub kernel_stack_top: VirtAddr,
    pub threads: Vec<Weak<Mutex<Thread>>>,
    pub objects: Vec<Option<Arc<dyn Object>>>,
    pub current_directory: AbsolutePath,
    pub exit_code: Option<u64>,
    pub parent: Option<ProcessRef>,
    pub signal_actions: Vec<SignalAction>,
    pub pending_signals: Signals,
    pub blocked_signals: Signals,
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Process {
            blocked_signals: Signals::default(),
            pending_signals: Signals::default(),
            signal_actions: default_signal_action_vec(),
            pid: ProcessID::default(),
            current_directory: AbsolutePath::default(),
            addrspace: AddrSpace::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            objects: Vec::new(),
            exit_code: None,
            parent: None,
        }))
    }
}
