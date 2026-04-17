use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use seele_sys::signal::Signals;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::filesystem::absolute_path::AbsolutePath;
use crate::memory::addrspace::AddrSpace;
use crate::misc::timer::Timer;
use crate::process::group::ProcessGroupID;
use crate::signal::action::SignalAction;
use crate::signal::misc::default_signal_action_vec;
use crate::{object::Object, process::misc::ProcessID, thread::thread::Thread};

pub mod execve;
pub mod fork;
pub mod group;
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
    pub group_id: ProcessGroupID,
    pub timers: Vec<Option<Timer>>,
    pub program_break: u64,
}

impl Default for Process {
    fn default() -> Self {
        Process {
            group_id: ProcessGroupID::default(),
            pending_signals: Signals::default(),
            signal_actions: default_signal_action_vec(),
            program_break: 0,
            pid: ProcessID::default(),
            current_directory: AbsolutePath::default(),
            addrspace: AddrSpace::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            objects: Vec::new(),
            exit_code: None,
            parent: None,
            timers: Vec::new(),
        }
    }
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Self::default()))
    }
}
