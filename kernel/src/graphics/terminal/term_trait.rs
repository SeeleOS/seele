use core::fmt::Debug;

use crate::graphics::object_config::WindowSizeInfo;

pub trait AbstractTerminal: Debug + Sync + Send {
    fn push_str(&mut self, str: &str);
    fn size(&self) -> WindowSizeInfo;
}
