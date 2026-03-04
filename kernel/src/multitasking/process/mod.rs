use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use elfloader::ElfBinary;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::filesystem::path::Path;
use crate::{
    memory::page_table_wrapper::PageTableWrapped,
    misc::stack_builder::StackBuilder,
    multitasking::{
        memory::{allocate_kernel_stack, allocate_stack},
        process::misc::{ProcessID, init_objects, init_stack_layout},
        thread::{
            THREAD_MANAGER,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
            thread::Thread,
        },
    },
    object::Object,
    userspace::elf_loader::load_elf,
};

pub mod manager;
pub mod misc;
pub mod new;

pub type ProcessRef = Arc<Mutex<Process>>;

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub page_table: PageTableWrapped,
    pub kernel_stack_top: VirtAddr,
    pub threads: Vec<Weak<Mutex<Thread>>>,
    pub objects: Vec<Arc<dyn Object>>,
    pub current_directory: Path,
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Process {
            pid: ProcessID::default(),
            current_directory: Path::default(),
            page_table: PageTableWrapped::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            objects: Vec::new(),
        }))
    }
}
