use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    filesystem::{
        info::{FileLikeInfo, LinuxStat, UnixPermission},
        vfs_traits::FileLikeType,
    },
    impl_cast_function, impl_cast_function_non_trait,
    object::{FileFlags, Object, misc::ObjectResult, open_state::OpenState, traits::Statable},
};

const NEXT_DYNAMIC_NAMESPACE_INO_START: u64 = 0xF100_0000;
static NEXT_DYNAMIC_NAMESPACE_INO: AtomicU64 = AtomicU64::new(NEXT_DYNAMIC_NAMESPACE_INO_START);

pub type NamespaceRef = Arc<NamespaceObject>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceKind {
    Ipc,
    Mnt,
    Pid,
    User,
    Uts,
}

#[derive(Debug)]
pub struct NamespaceObject {
    kind: NamespaceKind,
    inode: u64,
    open_state: OpenState,
}

impl NamespaceObject {
    pub fn new(kind: NamespaceKind, inode: u64) -> NamespaceRef {
        Arc::new(Self {
            kind,
            inode,
            open_state: OpenState::default(),
        })
    }

    pub fn dynamic(kind: NamespaceKind) -> NamespaceRef {
        Self::new(
            kind,
            NEXT_DYNAMIC_NAMESPACE_INO.fetch_add(1, Ordering::Relaxed),
        )
    }

    pub fn kind(&self) -> NamespaceKind {
        self.kind
    }

    pub fn inode(&self) -> u64 {
        self.inode
    }

    fn name(&self) -> &'static str {
        match self.kind {
            NamespaceKind::Ipc => "ipc",
            NamespaceKind::Mnt => "mnt",
            NamespaceKind::Pid => "pid",
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

impl Object for NamespaceObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("namespace", NamespaceObject);
}
