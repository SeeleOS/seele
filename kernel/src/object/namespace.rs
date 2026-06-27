use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    filesystem::{
        info::{FileLikeInfo, LinuxStat, UnixPermission},
        vfs_traits::FileLikeType,
    },
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        open_state::OpenState,
        traits::{Configuratable, Statable},
    },
    process::{FdFlags, manager::get_current_process},
};

const NEXT_DYNAMIC_NAMESPACE_INO_START: u64 = 0xF100_0000;
const NS_GET_PARENT: u64 = 0xb702;
const NS_GET_USERNS: u64 = 0xb701;
static NEXT_DYNAMIC_NAMESPACE_INO: AtomicU64 = AtomicU64::new(NEXT_DYNAMIC_NAMESPACE_INO_START);

pub type NamespaceRef = Arc<NamespaceObject>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceKind {
    Ipc,
    Mnt,
    Pid,
    Time,
    User,
    Uts,
}

#[derive(Debug)]
pub struct NamespaceObject {
    kind: NamespaceKind,
    inode: u64,
    parent_inode: Option<u64>,
    user_inode: Option<u64>,
    open_state: OpenState,
}

impl NamespaceObject {
    pub fn new(kind: NamespaceKind, inode: u64) -> NamespaceRef {
        Arc::new(Self {
            kind,
            inode,
            parent_inode: None,
            user_inode: None,
            open_state: OpenState::default(),
        })
    }

    pub fn dynamic(kind: NamespaceKind) -> NamespaceRef {
        Self::dynamic_with_parent(kind, None, None)
    }

    pub fn dynamic_with_parent(
        kind: NamespaceKind,
        parent: Option<&NamespaceRef>,
        user: Option<&NamespaceRef>,
    ) -> NamespaceRef {
        Arc::new(Self {
            kind,
            inode: NEXT_DYNAMIC_NAMESPACE_INO.fetch_add(1, Ordering::Relaxed),
            parent_inode: parent.map(|namespace| namespace.inode()),
            user_inode: user.map(|namespace| namespace.inode()),
            open_state: OpenState::default(),
        })
    }

    pub fn kind(&self) -> NamespaceKind {
        self.kind
    }

    pub fn inode(&self) -> u64 {
        self.inode
    }

    pub fn parent_inode(&self) -> Option<u64> {
        self.parent_inode
    }

    pub fn user_inode(&self) -> Option<u64> {
        self.user_inode
    }

    fn name(&self) -> &'static str {
        match self.kind {
            NamespaceKind::Ipc => "ipc",
            NamespaceKind::Mnt => "mnt",
            NamespaceKind::Pid => "pid",
            NamespaceKind::Time => "time",
            NamespaceKind::User => "user",
            NamespaceKind::Uts => "uts",
        }
    }
}

impl Statable for NamespaceObject {
    fn stat(&self) -> LinuxStat {
        FileLikeInfo::new(
            String::from(self.name()),
            0,
            UnixPermission(0o444),
            FileLikeType::File,
        )
        .with_inode(self.inode)
        .as_linux()
    }
}

impl Configuratable for NamespaceObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        let ConfigurateRequest::RawIoctl { request, .. } = request else {
            return Err(ObjectError::InvalidRequest);
        };

        let target_inode = match request {
            NS_GET_PARENT => self.parent_inode,
            NS_GET_USERNS => self.user_inode,
            _ => return Err(ObjectError::InvalidRequest),
        }
        .ok_or(ObjectError::InvalidRequest)?;

        let namespace = crate::process::manager::MANAGER
            .lock()
            .processes
            .values()
            .find_map(|process| {
                let process = process.lock();
                [
                    process.ipc_namespace.clone(),
                    process.mnt_namespace.clone(),
                    process.pid_namespace.clone(),
                    process.time_namespace.clone(),
                    process.user_namespace.clone(),
                    process.uts_namespace.clone(),
                ]
                .into_iter()
                .find(|namespace| namespace.inode() == target_inode)
            })
            .ok_or(ObjectError::InvalidRequest)?;

        let fd = get_current_process()
            .lock()
            .push_object_with_flags(namespace, FdFlags::CLOEXEC);
        Ok(fd as isize)
    }
}

impl Object for NamespaceObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function!("statable", Statable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function_non_trait!("namespace", NamespaceObject);
}
