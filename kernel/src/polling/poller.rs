use crate::object::Object;

#[derive(Debug)]
pub struct PollerObject {}

impl PollerObject {
    pub fn new() -> Self {
        Self {}
    }
}

impl Object for PollerObject {}
