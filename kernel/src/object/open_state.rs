use spin::Mutex;

use crate::object::FileFlags;

#[derive(Debug, Default)]
pub struct OpenState {
    flags: Mutex<FileFlags>,
}

impl OpenState {
    pub fn new(flags: FileFlags) -> Self {
        Self {
            flags: Mutex::new(flags),
        }
    }

    pub fn get_flags(&self) -> FileFlags {
        *self.flags.lock()
    }

    pub fn set_flags(&self, flags: FileFlags) {
        *self.flags.lock() = flags;
    }

    pub fn contains(&self, flags: FileFlags) -> bool {
        self.flags.lock().contains(flags)
    }
}
