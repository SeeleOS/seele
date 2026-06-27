use crate::memory::utils::Mut;
use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use bitflags::bitflags;
use num_enum::TryFromPrimitive;
use x86_64::VirtAddr;

use crate::filesystem::{path::Path, vfs_traits::MountFlags};
use crate::ipc::sysv_shm::ProcessShmMapping;
use crate::memory::addrspace::AddrSpace;
use crate::misc::timer::Timer;
use crate::net::namespace::{NetNamespace, NetNamespaceRef};
use crate::object::namespace::{NamespaceKind, NamespaceObject, NamespaceRef};
use crate::object::{misc::ObjectRef, pipe::PipeEndpoint};
use crate::process::group::{ProcessGroupID, SessionID};
use crate::signal::misc::default_signal_action_vec;
use crate::signal::{PendingSignalInfo, SIGNAL_AMOUNT, Signal, Signals, action::SignalAction};
use crate::thread::misc::ThreadID;
use crate::{process::misc::ProcessID, thread::thread::Thread};
use fd_table::FdTableRef;
use fs_context::FsContextRef;

pub mod acct;
pub mod execve;
pub mod fd_table;
pub mod fork;
pub mod fs_context;
pub mod group;
pub mod manager;
pub mod misc;
pub mod new;
pub mod object;
pub mod ptrace;
pub mod wait;

#[cfg(test)]
mod test;

pub type ProcessRef = Arc<Mut<Process>>;
pub use fd_table::{FdTable, clone_fd_table, new_fd_table};
pub use fs_context::{FsContext, clone_fs_context, new_fs_context};

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControllingTerminal(pub u64);

pub const CAP_LAST_CAP: u32 = 40;
pub const DEFAULT_CAPABILITY_SET: [u32; 2] = [u32::MAX, (1u32 << (CAP_LAST_CAP - 31)) - 1];
const DEFAULT_RLIMIT_NOFILE: u64 = 1024;
const DEFAULT_RLIMIT_MEMLOCK: u64 = 8 * 1024 * 1024;
const DEFAULT_RLIMIT_STACK_CUR: u64 = 8 * 1024 * 1024;
const DEFAULT_RLIMIT_STACK_MAX: u64 = u64::MAX;
const DEFAULT_RLIMIT_CORE: u64 = 0;
const DEFAULT_RLIMIT_FSIZE: u64 = u64::MAX;
const DEFAULT_RLIMIT_NPROC: u64 = 4096;
const DEFAULT_RLIMIT_DATA: u64 = 64 * 1024 * 1024;
const CLD_EXITED: i32 = 1;
const CLD_KILLED: i32 = 2;

bitflags! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct FdFlags: u32 {
        const CLOEXEC = 1 << 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, TryFromPrimitive)]
#[repr(i32)]
pub enum LinuxSchedPolicy {
    Other = 0,
    Fifo = 1,
    RoundRobin = 2,
    Batch = 3,
    Idle = 5,
    Deadline = 6,
}

impl LinuxSchedPolicy {
    pub fn min_priority(self) -> i32 {
        match self {
            Self::Fifo | Self::RoundRobin => 1,
            Self::Other | Self::Batch | Self::Idle | Self::Deadline => 0,
        }
    }

    pub fn max_priority(self) -> i32 {
        match self {
            Self::Fifo | Self::RoundRobin => 99,
            Self::Other | Self::Batch | Self::Idle | Self::Deadline => 0,
        }
    }
}

#[derive(Debug)]
struct PipeFdReference {
    pipe: Arc<PipeEndpoint>,
}

impl PipeFdReference {
    fn new(pipe: Arc<PipeEndpoint>) -> Self {
        pipe.clone_fd_reference();
        Self { pipe }
    }
}

impl Clone for PipeFdReference {
    fn clone(&self) -> Self {
        Self::new(self.pipe.clone())
    }
}

impl Drop for PipeFdReference {
    fn drop(&mut self) {
        self.pipe.close_fd_reference();
    }
}

#[derive(Clone, Debug)]
pub struct FdEntry {
    pub object: ObjectRef,
    pub fd_flags: FdFlags,
    pub created_by_open: bool,
    _pipe_reference: Option<PipeFdReference>,
}

impl FdEntry {
    pub fn new(object: ObjectRef, fd_flags: FdFlags) -> Self {
        Self::with_created_by_open(object, fd_flags, false)
    }

    pub fn with_created_by_open(
        object: ObjectRef,
        fd_flags: FdFlags,
        created_by_open: bool,
    ) -> Self {
        let pipe_reference = object.clone().as_pipe().ok().map(PipeFdReference::new);
        Self {
            object,
            fd_flags,
            created_by_open,
            _pipe_reference: pipe_reference,
        }
    }
}

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub addrspace: AddrSpace,
    pub kernel_stack_top: VirtAddr,
    pub threads: Vec<Weak<Mut<Thread>>>,
    pub fd_table: FdTableRef,
    pub fs_context: FsContextRef,
    pub exec_path: Path,
    pub command_line: Vec<String>,
    pub exit_status: Option<ProcessExitStatus>,
    pub parent: Option<ProcessRef>,
    pub signal_actions: Vec<SignalAction>,
    pub pending_signals: Signals,
    pub pending_signal_info: Vec<Option<PendingSignalInfo>>,
    pub group_id: ProcessGroupID,
    pub session_id: SessionID,
    pub controlling_terminal: Option<ControllingTerminal>,
    pub timers: Vec<Option<Timer>>,
    pub program_break: u64,
    pub program_break_base: u64,
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
    pub sched_policy: LinuxSchedPolicy,
    pub sched_priority: i32,
    pub secure_bits: u32,
    pub rlimit_nofile_cur: u64,
    pub rlimit_nofile_max: u64,
    pub rlimit_memlock_cur: u64,
    pub rlimit_memlock_max: u64,
    pub rlimit_rtprio_cur: u64,
    pub rlimit_rtprio_max: u64,
    pub rlimit_core_cur: u64,
    pub rlimit_core_max: u64,
    pub rlimit_fsize_cur: u64,
    pub rlimit_fsize_max: u64,
    pub rlimit_nproc_cur: u64,
    pub rlimit_nproc_max: u64,
    pub rlimit_data_cur: u64,
    pub rlimit_data_max: u64,
    pub rlimit_stack_cur: u64,
    pub rlimit_stack_max: u64,
    pub thread_keyring: i32,
    pub process_keyring: i32,
    pub session_keyring: i32,
    pub user_keyring: i32,
    pub request_key_default_keyring: i32,
    pub request_key_auth_key: i32,
    pub request_key_requested_key: i32,
    pub request_key_requestor_keyring: i32,
    pub capability_effective: [u32; 2],
    pub capability_permitted: [u32; 2],
    pub capability_inheritable: [u32; 2],
    pub capability_bounding: [u32; 2],
    pub capability_ambient: [u32; 2],
    pub child_subreaper: bool,
    pub parent_death_signal: Option<Signal>,
    pub child_exit_signal: Signal,
    pub dumpable: bool,
    pub no_new_privs: bool,
    pub net_namespace: NetNamespaceRef,
    pub ipc_namespace: NamespaceRef,
    pub mnt_namespace: NamespaceRef,
    pub pid_namespace: NamespaceRef,
    pub pid_namespace_local_pid: Option<u64>,
    pub pid_namespace_parent_inode: Option<u64>,
    pub pending_child_pid_namespace: Option<NamespaceRef>,
    pub user_namespace: NamespaceRef,
    pub uts_namespace: NamespaceRef,
    pub mount_namespace_snapshot: Option<Vec<u64>>,
    pub mount_namespace_flag_overrides: BTreeMap<u64, MountFlags>,
    pub mount_namespace_shared_with_parent: bool,
    pub sysv_shm_mappings: Vec<ProcessShmMapping>,
    pub vfork_blocker: Option<ThreadID>,
    pub borrowed_addrspace_from_parent: bool,
    pub ptrace: ptrace::PtraceState,
    pub wait_event: Option<wait::ProcessWaitEvent>,
}

impl Default for Process {
    fn default() -> Self {
        Process {
            group_id: ProcessGroupID::default(),
            session_id: SessionID::default(),
            controlling_terminal: None,
            pending_signals: Signals::default(),
            pending_signal_info: alloc::vec![None; SIGNAL_AMOUNT],
            signal_actions: default_signal_action_vec(),
            program_break: 0,
            program_break_base: 0,
            pid: ProcessID::default(),
            addrspace: AddrSpace::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            fd_table: fd_table::new_fd_table(),
            fs_context: fs_context::new_fs_context(),
            exec_path: Path::new(""),
            command_line: Vec::new(),
            exit_status: None,
            parent: None,
            timers: Vec::new(),
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
            sched_policy: LinuxSchedPolicy::Other,
            sched_priority: 0,
            secure_bits: 0,
            rlimit_nofile_cur: DEFAULT_RLIMIT_NOFILE,
            rlimit_nofile_max: DEFAULT_RLIMIT_NOFILE,
            rlimit_memlock_cur: DEFAULT_RLIMIT_MEMLOCK,
            rlimit_memlock_max: DEFAULT_RLIMIT_MEMLOCK,
            rlimit_rtprio_cur: 0,
            rlimit_rtprio_max: 0,
            rlimit_core_cur: DEFAULT_RLIMIT_CORE,
            rlimit_core_max: DEFAULT_RLIMIT_CORE,
            rlimit_fsize_cur: DEFAULT_RLIMIT_FSIZE,
            rlimit_fsize_max: DEFAULT_RLIMIT_FSIZE,
            rlimit_nproc_cur: DEFAULT_RLIMIT_NPROC,
            rlimit_nproc_max: DEFAULT_RLIMIT_NPROC,
            rlimit_data_cur: DEFAULT_RLIMIT_DATA,
            rlimit_data_max: DEFAULT_RLIMIT_DATA,
            rlimit_stack_cur: DEFAULT_RLIMIT_STACK_CUR,
            rlimit_stack_max: DEFAULT_RLIMIT_STACK_MAX,
            thread_keyring: 0,
            process_keyring: 0,
            session_keyring: 0,
            user_keyring: 0,
            request_key_default_keyring: 0,
            request_key_auth_key: 0,
            request_key_requested_key: 0,
            request_key_requestor_keyring: 0,
            capability_effective: DEFAULT_CAPABILITY_SET,
            capability_permitted: DEFAULT_CAPABILITY_SET,
            capability_inheritable: [0; 2],
            capability_bounding: DEFAULT_CAPABILITY_SET,
            capability_ambient: [0; 2],
            child_subreaper: false,
            parent_death_signal: None,
            child_exit_signal: Signal::SIGCHLD,
            dumpable: true,
            no_new_privs: false,
            net_namespace: NetNamespace::init(),
            ipc_namespace: NamespaceObject::new(NamespaceKind::Ipc, 0xEFFF_FFFF),
            mnt_namespace: NamespaceObject::new(NamespaceKind::Mnt, 0xEFFF_FFF8),
            pid_namespace: NamespaceObject::new(NamespaceKind::Pid, 0xEFFF_FFFC),
            pid_namespace_local_pid: None,
            pid_namespace_parent_inode: None,
            pending_child_pid_namespace: None,
            user_namespace: NamespaceObject::new(NamespaceKind::User, 0xEFFF_FFFD),
            uts_namespace: NamespaceObject::new(NamespaceKind::Uts, 0xEFFF_FFFE),
            mount_namespace_snapshot: None,
            mount_namespace_flag_overrides: BTreeMap::new(),
            mount_namespace_shared_with_parent: true,
            sysv_shm_mappings: Vec::new(),
            vfork_blocker: None,
            borrowed_addrspace_from_parent: false,
            ptrace: ptrace::PtraceState::default(),
            wait_event: None,
        }
    }
}

impl Process {
    pub fn update_uid_capabilities(&mut self, old_effective_uid: u32) {
        if self.effective_uid == 0 {
            self.capability_effective = self.capability_permitted;
        } else if old_effective_uid == 0 {
            self.capability_effective = [0; 2];
        }

        if self.real_uid != 0
            && self.effective_uid != 0
            && self.saved_uid != 0
            && !self.keep_capabilities
        {
            self.capability_permitted = [0; 2];
            self.capability_ambient = [0; 2];
        }
    }

    pub fn update_exec_uid_capabilities(&mut self, old_effective_uid: u32, gained_root: bool) {
        if gained_root && !self.no_new_privs {
            self.capability_permitted = self.capability_bounding;
            self.capability_effective = self.capability_permitted;
            self.capability_ambient = [0; 2];
            return;
        }

        self.update_uid_capabilities(old_effective_uid);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitStatus {
    Exited(u8),
    Signaled { signal: Signal, core_dumped: bool },
}

impl ProcessExitStatus {
    pub fn from_exit_code(code: u64) -> Self {
        Self::Exited((code & 0xff) as u8)
    }

    pub fn from_signal(signal: Signal) -> Self {
        Self::Signaled {
            signal,
            core_dumped: false,
        }
    }

    pub fn wait_status(self) -> i32 {
        match self {
            Self::Exited(code) => i32::from(code) << 8,
            Self::Signaled {
                signal,
                core_dumped,
            } => signal as i32 | if core_dumped { 0x80 } else { 0 },
        }
    }

    pub fn waitid_code(self) -> i32 {
        match self {
            Self::Exited(_) => CLD_EXITED,
            Self::Signaled { .. } => CLD_KILLED,
        }
    }

    pub fn waitid_status(self) -> i32 {
        match self {
            Self::Exited(code) => i32::from(code),
            Self::Signaled { signal, .. } => signal as i32,
        }
    }
}

impl Process {
    pub fn fs_owner_ids(&self) -> (u32, u32) {
        (self.fs_uid, self.fs_gid)
    }

    pub fn empty() -> ProcessRef {
        Arc::new(Mut::new(Self::default()))
    }

    pub fn stdin_terminal_rdev(&self) -> Option<u64> {
        let object = {
            let fd_table = self.fd_table.lock();
            fd_table.first()?.as_ref()?.object.clone()
        };
        let device = object
            .clone()
            .as_file_like()
            .ok()
            .and_then(|file| file.device_backing_object())
            .unwrap_or(object);
        let is_terminal =
            device.clone().as_tty_device().is_ok() || device.clone().as_pty_slave().is_ok();
        if !is_terminal {
            return None;
        }

        {
            let fd_table = self.fd_table.lock();
            fd_table.first()?.as_ref()?.object.clone()
        }
        .as_statable()
        .ok()
        .map(|statable| statable.stat().st_rdev)
    }

    pub fn stdin_foreground_process_group(&self) -> Option<ProcessGroupID> {
        let object = {
            let fd_table = self.fd_table.lock();
            fd_table.first()?.as_ref()?.object.clone()
        };
        let device = object
            .clone()
            .as_file_like()
            .ok()
            .and_then(|file| file.device_backing_object())
            .unwrap_or(object);
        if let Ok(tty) = device.clone().as_tty_device() {
            return tty.foreground_process_group();
        }
        if let Ok(pty) = device.as_pty_slave() {
            return pty.foreground_process_group();
        }
        None
    }
}
