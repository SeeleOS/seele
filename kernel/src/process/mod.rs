use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use bitflags::bitflags;
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

const CAP_LAST_CAP: u32 = 40;
const DEFAULT_CAPABILITY_LOW: u32 = u32::MAX;
const DEFAULT_CAPABILITY_HIGH: u32 = (1u32 << (CAP_LAST_CAP - 31)) - 1;

bitflags! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct FdFlags: u32 {
        const CLOEXEC = 1 << 0;
    }
}

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub addrspace: AddrSpace,
    pub kernel_stack_top: VirtAddr,
    pub threads: Vec<Weak<Mutex<Thread>>>,
    pub objects: Vec<Option<Arc<dyn Object>>>,
    pub object_flags: Vec<FdFlags>,
    pub current_directory: AbsolutePath,
    pub command_line: Vec<String>,
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
    pub user_namespace_uid_map: Option<String>,
    pub user_namespace_gid_map: Option<String>,
    pub user_namespace_setgroups: Option<String>,
    pub keep_capabilities: bool,
    pub oom_score_adj: i32,
    pub secure_bits: u32,
    pub session_keyring: i32,
    pub user_keyring: i32,
    pub capability_effective: [u32; 2],
    pub capability_permitted: [u32; 2],
    pub capability_inheritable: [u32; 2],
    pub capability_ambient: [u32; 2],
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
            object_flags: Vec::new(),
            command_line: Vec::new(),
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
            user_namespace_uid_map: None,
            user_namespace_gid_map: None,
            user_namespace_setgroups: None,
            keep_capabilities: false,
            oom_score_adj: 0,
            secure_bits: 0,
            session_keyring: 0,
            user_keyring: 0,
            capability_effective: [DEFAULT_CAPABILITY_LOW, DEFAULT_CAPABILITY_HIGH],
            capability_permitted: [DEFAULT_CAPABILITY_LOW, DEFAULT_CAPABILITY_HIGH],
            capability_inheritable: [0; 2],
            capability_ambient: [0; 2],
        }
    }
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Self::default()))
    }
}
