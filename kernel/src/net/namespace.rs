use alloc::{string::String, sync::Arc};
use core::sync::atomic::{AtomicU64, Ordering};

use lazy_static::lazy_static;

use crate::{
    filesystem::{
        info::{FileLikeInfo, LinuxStat, UnixPermission},
        vfs_traits::FileLikeType,
    },
    impl_cast_function, impl_cast_function_non_trait,
    object::{Object, traits::Statable},
};

const PROC_NET_INIT_INO: u64 = 0xEFFF_FFF9;
const NEXT_DYNAMIC_NET_NAMESPACE_INO_START: u64 = 0xF000_0000;

static NEXT_DYNAMIC_NET_NAMESPACE_INO: AtomicU64 =
    AtomicU64::new(NEXT_DYNAMIC_NET_NAMESPACE_INO_START);

lazy_static! {
    static ref INIT_NET_NAMESPACE: Arc<NetNamespace> = Arc::new(NetNamespace {
        inode: PROC_NET_INIT_INO,
    });
}

pub type NetNamespaceRef = Arc<NetNamespace>;

#[derive(Debug)]
pub struct NetNamespace {
    inode: u64,
}

impl NetNamespace {
    pub fn init() -> NetNamespaceRef {
        INIT_NET_NAMESPACE.clone()
    }

    pub fn new() -> NetNamespaceRef {
        Arc::new(Self {
            inode: NEXT_DYNAMIC_NET_NAMESPACE_INO.fetch_add(1, Ordering::Relaxed),
        })
    }

    pub fn inode(&self) -> u64 {
        self.inode
    }
}

impl Statable for NetNamespace {
    fn stat(&self) -> LinuxStat {
        FileLikeInfo::new(
            String::from("net"),
            0,
            UnixPermission(0o444),
            FileLikeType::File,
        )
        .with_inode(self.inode)
        .as_linux()
    }
}

impl Object for NetNamespace {
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("net_namespace", NetNamespace);
}
