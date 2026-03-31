use alloc::{collections::BTreeMap, string::String, sync::Arc};
use lazy_static::lazy_static;
use spin::Mutex;

use super::UnixListenerInner;

lazy_static! {
    pub static ref UNIX_SOCKET_REGISTRY: Mutex<BTreeMap<String, Option<Arc<UnixListenerInner>>>> =
        Mutex::new(BTreeMap::new());
}
