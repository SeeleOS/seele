use x86_64::VirtAddr;

use crate::memory::addrspace::AddrSpace;

impl AddrSpace {
    pub fn fetch_add_user_mem(&mut self, pages: u64) -> VirtAddr {
        let mem = self.user_mem;
        self.user_mem += (pages + 1) * 4096;
        mem
    }
}
