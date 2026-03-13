use elfloader::ElfBinary;
use x86_64::VirtAddr;
use xmas_elf::program;

use crate::misc::aux::AuxType;

#[derive(Debug)]
pub struct StackBuilder {
    sp: VirtAddr,
    write_sp: *mut u64,
}

impl StackBuilder {
    pub fn new(sp: u64, write_sp: *mut u64) -> Self {
        Self {
            sp: VirtAddr::new(sp),
            write_sp,
        }
    }

    pub fn push_aux_entries(&mut self, file: &ElfBinary) {
        let mut base = 0;

        for ele in file.program_headers() {
            if matches!(ele.get_type().unwrap(), program::Type::Load) && base == 0 {
                base = ele.virtual_addr();
            }
        }

        log::trace!("stack_builder base: {base}");

        self.push_aux_entry(AuxType::Null, 0);
        self.push_aux_entry(AuxType::EntryPointAddress, file.entry_point());
        self.push_aux_entry(
            AuxType::ProgramHeaderAmount,
            file.program_headers().count() as u64,
        );
        self.push_aux_entry(AuxType::ProgramHeaderEntrySize, 56);
        self.push_aux_entry(
            AuxType::ProgramHeaderTable,
            base + file.file.header.pt2.ph_offset(),
        );
        self.push_aux_entry(AuxType::PageSize, 4096);
    }

    fn push_aux_entry(&mut self, aux_type: AuxType, value: u64) {
        self.push(value);
        self.push(aux_type as u64);
    }

    pub fn push(&mut self, value: u64) {
        unsafe { write_and_sub(&mut self.write_sp, value) };
        self.sp -= 8;
    }

    pub fn push_str(&mut self, s: &str) -> u64 {
        let bytes = s.as_bytes();
        let len_with_null = (bytes.len() + 1) as u64;

        // 1. 移动用户态 SP (虚拟地址)
        self.sp -= len_with_null * 8;
        // 2. 移动内核态写入指针 (物理映射地址)
        self.write_sp = unsafe { self.write_sp.sub(len_with_null as usize) };

        let user_vaddr = self.sp;
        let kernel_vaddr = self.write_sp;

        unsafe {
            // 关键：将内核写入地址强转为 *mut u8
            let slice =
                core::slice::from_raw_parts_mut(kernel_vaddr as *mut u8, len_with_null as usize);

            // 这里的 bytes 是 &[u8]，slice 现在也是 &mut [u8]，匹配成功！
            slice[..bytes.len()].copy_from_slice(bytes);
            slice[bytes.len()] = 0; // 写入 \0
        }

        // 返回用户态看到的地址，这个地址会被存入 argv[0]
        user_vaddr.as_u64()
    }

    pub fn finish(self) -> VirtAddr {
        if !self.sp.is_aligned(16u64) {
            log::warn!("Stack pointer is not 16 byte aligned");
        }
        self.sp
    }
}

/// # Safety
/// Must provide valid pointer
unsafe fn write_and_sub(ptr: &mut *mut u64, data: u64) {
    unsafe {
        *ptr = ptr.sub(1);
        ptr.write(data);
    }
}
