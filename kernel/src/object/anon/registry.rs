use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};

pub(super) struct WatcherRegistry<T> {
    watchers: BTreeMap<u64, Vec<Weak<T>>>,
}

impl<T> Default for WatcherRegistry<T> {
    fn default() -> Self {
        Self {
            watchers: BTreeMap::new(),
        }
    }
}

impl<T> WatcherRegistry<T> {
    pub(super) fn register(&mut self, key: u64, object: &Arc<T>) {
        let watchers = self.watchers.entry(key).or_default();
        watchers.retain(|watcher| watcher.strong_count() > 0);
        watchers.push(Arc::downgrade(object));
    }

    pub(super) fn live_watchers(&mut self, key: u64) -> Vec<Arc<T>> {
        let Some(watchers) = self.watchers.get_mut(&key) else {
            return Vec::new();
        };

        let mut strong = Vec::new();
        watchers.retain(|watcher| {
            if let Some(object) = watcher.upgrade() {
                strong.push(object);
                true
            } else {
                false
            }
        });
        strong
    }
}
