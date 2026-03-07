use acpi::address::AddressSpace;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use bootloader_api::info::MemoryRegion;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::filesystem::path::Path;
use crate::memory::addrspace::AddrSpace;
use crate::{
    memory::page_table_wrapper::PageTableWrapped,
    multitasking::{process::misc::ProcessID, thread::thread::Thread},
    object::Object,
};

pub mod execve;
pub mod fork;
pub mod manager;
pub mod misc;
pub mod new;

pub type ProcessRef = Arc<Mutex<Process>>;

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub addrspace: AddrSpace,
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
            addrspace: AddrSpace::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            objects: Vec::new(),
        }))
    }
}
