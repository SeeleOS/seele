use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::{Object, traits::{Controllable, Readable, Writable}},
    polling::object::Pollable,
};

use super::UnixSocketObject;

impl Object for UnixSocketObject {
    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("controllable", Controllable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function_non_trait!("unix_socket", UnixSocketObject);
}
