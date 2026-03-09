use core::sync::atomic::AtomicU64;

use alloc::{sync::Arc, vec::Vec};
use elfloader::ElfBinary;

use crate::{
    misc::stack_builder::StackBuilder,
    object::{Object, tty_device::TtyDevice},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

impl Default for ProcessID {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

pub fn init_objects(objects: &mut Vec<Option<Arc<dyn Object>>>) {
    objects.push(Some(Arc::new(TtyDevice))); // stdin (unimpllemented)
    objects.push(Some(Arc::new(TtyDevice))); // stdout
    objects.push(Some(Arc::new(TtyDevice))); // stderr
}

pub fn init_stack_layout(builder: &mut StackBuilder, file: &ElfBinary) {
    // A. 先在栈的最顶端存入字符串 "init\0"
    // 字符串占用 5 字节，为了对齐我们按 8 字节处理
    let arg_str = builder.push_str("init");

    // 手动移动指针存入字符串
    //*virt_stack_write = (virt_stack_write).sub(16);
    //core::ptr::copy_nonoverlapping(arg_str.as_ptr(), *virt_stack_write as *mut u8, str_len);

    builder.push(0);
    // B. 使用你的 write_and_sub 按照 ABI 逆序压栈
    builder.push_aux_entries(file);

    builder.push(0); // envp = 0

    // argv
    builder.push(0); // argv[1] == null (end)
    builder.push(arg_str); // argv[0] ==  *arg_str

    // argc (1 arguments)
    builder.push(1);
}
