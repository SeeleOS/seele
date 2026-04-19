use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::filesystem::absolute_path::AbsolutePath;
use crate::memory::addrspace::AddrSpace;
use crate::misc::timer::Timer;
use crate::process::group::ProcessGroupID;
use crate::signal::misc::default_signal_action_vec;
use crate::signal::{Signals, action::SignalAction};
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
    pub file_mode_creation_mask: u32,
    pub real_uid: u32,
    pub effective_uid: u32,
    pub saved_uid: u32,
    pub fs_uid: u32,
    pub real_gid: u32,
    pub effective_gid: u32,
    pub saved_gid: u32,
    pub fs_gid: u32,
    pub supplementary_groups: Vec<u32>,
    pub keep_capabilities: bool,
    pub capability_effective: [u32; 2],
    pub capability_permitted: [u32; 2],
    pub capability_inheritable: [u32; 2],
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
            file_mode_creation_mask: 0,
            real_uid: 0,
            effective_uid: 0,
            saved_uid: 0,
            fs_uid: 0,
            real_gid: 0,
            effective_gid: 0,
            saved_gid: 0,
            fs_gid: 0,
            supplementary_groups: Vec::new(),
            keep_capabilities: false,
            capability_effective: [u32::MAX; 2],
            capability_permitted: [u32::MAX; 2],
            capability_inheritable: [0; 2],
        }
    }
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Self::default()))
    }
}
