use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        FileFlags, Object,
        traits::{Readable, Writable},
    },
    polling::object::Pollable,
};

use super::UnixSocketObject;

impl Object for UnixSocketObject {
    fn get_flags(self: alloc::sync::Arc<Self>) -> crate::object::misc::ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(
        self: alloc::sync::Arc<Self>,
        flags: FileFlags,
    ) -> crate::object::misc::ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function_non_trait!("unix_socket", UnixSocketObject);
}
