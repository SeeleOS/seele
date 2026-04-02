use crate::{
    impl_cast_function,
    object::{
        Object,
        traits::{Readable, Writable},
    },
};

#[derive(Debug)]
pub struct DevNull;

impl Object for DevNull {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
}

impl Writable for DevNull {
    fn write(&self, buffer: &[u8]) -> crate::object::misc::ObjectResult<usize> {
        Ok(buffer.len())
    }
}

impl Readable for DevNull {
    fn read(&self, buffer: &mut [u8]) -> crate::object::misc::ObjectResult<usize> {
        buffer.fill(0);
        Ok(buffer.len())
    }
}
