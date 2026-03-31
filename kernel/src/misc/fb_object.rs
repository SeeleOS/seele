use crate::{
    misc::framebuffer::FRAME_BUFFER,
    object::{
        Object,
        traits::{Readable, Writable},
    },
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FramebufferObject;

impl Object for FramebufferObject {}

impl Readable for FramebufferObject {
    fn read(&self, buffer: &mut [u8]) -> crate::object::misc::ObjectResult<usize> {
        todo!()
    }
}

impl Writable for FramebufferObject {
    fn write(&self, buffer: &[u8]) -> crate::object::misc::ObjectResult<usize> {
        todo!()
    }
}
