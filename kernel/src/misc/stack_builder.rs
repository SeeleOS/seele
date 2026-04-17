use alloc::vec::Vec;
use x86_64::VirtAddr;

use crate::elfloader::ElfInfo;
use crate::misc::auxv::AuxType;

#[derive(Debug)]
pub struct StackBuilder {
    sp: VirtAddr,
    top: VirtAddr,
    page_write_bases: Vec<u64>,
}

impl StackBuilder {
    pub fn new(top: u64, page_write_bases: Vec<u64>) -> Self {
        Self {
            sp: VirtAddr::new(top),
            top: VirtAddr::new(top),
            page_write_bases,
        }
    }

    pub fn push_aux_entries(
        &mut self,
        file: &ElfInfo,
        interpreter_base: Option<u64>,
        execfn: u64,
        platform: u64,
        random: u64,
    ) {
        self.push_aux_entry(AuxType::Null, 0);
        self.push_aux_entry(AuxType::ExecFilename, execfn);
        self.push_aux_entry(AuxType::Random, random);
        self.push_aux_entry(AuxType::Secure, 0);
        self.push_aux_entry(AuxType::ClockTick, 100);
        self.push_aux_entry(AuxType::HardwareCapabilities, 0);
        self.push_aux_entry(AuxType::Platform, platform);
        self.push_aux_entry(AuxType::EffectiveGroupId, 0);
        self.push_aux_entry(AuxType::GroupId, 0);
        self.push_aux_entry(AuxType::EffectiveUserId, 0);
        self.push_aux_entry(AuxType::UserId, 0);
        self.push_aux_entry(AuxType::NotElf, 0);
        self.push_aux_entry(AuxType::Flags, 0);
        self.push_aux_entry(AuxType::EntryPointAddress, file.entry_point);
        self.push_aux_entry(
            AuxType::ProgramHeaderAmount,
            file.program_header_count as u64,
        );
        self.push_aux_entry(
            AuxType::ProgramHeaderEntrySize,
            file.program_header_entry_size as u64,
        );
        self.push_aux_entry(AuxType::ProgramHeaderTable, file.program_header_table);
        if let Some(base) = interpreter_base {
            self.push_aux_entry(AuxType::BaseAddress, base);
        }
        self.push_aux_entry(AuxType::PageSize, 4096);
    }

    fn push_aux_entry(&mut self, aux_type: AuxType, value: u64) {
        self.push(value);
        self.push(aux_type as u64);
    }

    pub fn push(&mut self, value: u64) {
        self.sp -= 8;
        self.write_bytes(self.sp, &value.to_ne_bytes());
    }

    pub fn push_str(&mut self, s: &str) -> u64 {
        let bytes = s.as_bytes();
        let len_with_null = (bytes.len() + 1) as u64;

        self.sp -= len_with_null;
        let user_vaddr = self.sp;

        self.write_bytes(user_vaddr, bytes);
        self.zero_bytes(user_vaddr + bytes.len() as u64, 1);

        user_vaddr.as_u64()
    }

    pub fn push_struct<T: Copy>(&mut self, value: &T) -> u64 {
        let size = core::mem::size_of::<T>() as u64;

        self.sp -= size;
        let bytes =
            unsafe { core::slice::from_raw_parts((value as *const T).cast::<u8>(), size as usize) };
        self.write_bytes(self.sp, bytes);

        self.sp.as_u64()
    }

    pub fn align_down(&mut self, align: u64) {
        let misalignment = self.sp.as_u64() & (align - 1);
        if misalignment == 0 {
            return;
        }

        self.pad_bytes(misalignment);
    }

    pub fn align_for_pushes(&mut self, bytes_to_push: u64, align: u64) {
        let final_sp = self.sp.as_u64().wrapping_sub(bytes_to_push);
        let padding = final_sp & (align - 1);
        if padding != 0 {
            self.pad_bytes(padding);
        }
    }

    pub fn finish(self) -> VirtAddr {
        if !self.sp.is_aligned(16u64) {
            log::warn!("Stack pointer is not 16 byte aligned");
        }
        self.sp
    }
}

impl StackBuilder {
    fn pad_bytes(&mut self, bytes: u64) {
        self.sp -= bytes;
        self.zero_bytes(self.sp, bytes as usize);
    }

    fn zero_bytes(&self, start: VirtAddr, len: usize) {
        self.write_repeated_byte(start, len, 0);
    }

    fn write_repeated_byte(&self, mut start: VirtAddr, mut len: usize, value: u8) {
        while len > 0 {
            let (page_base, available) = self.page_ptr_and_available(start);
            let chunk = len.min(available);

            unsafe {
                core::ptr::write_bytes(page_base, value, chunk);
            }

            start += chunk as u64;
            len -= chunk;
        }
    }

    fn write_bytes(&self, mut start: VirtAddr, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let (page_base, available) = self.page_ptr_and_available(start);
            let chunk = bytes.len().min(available);

            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), page_base, chunk);
            }

            start += chunk as u64;
            bytes = &bytes[chunk..];
        }
    }

    fn page_ptr_and_available(&self, addr: VirtAddr) -> (*mut u8, usize) {
        let stack_bytes = self.page_write_bases.len() as u64 * 4096;
        let start = self.top - stack_bytes;
        assert!(addr >= start && addr < self.top, "stack write out of range");

        let offset = (addr - start) as usize;
        let page_index = offset / 4096;
        let offset_in_page = offset % 4096;
        let page_base = self.page_write_bases[page_index] as *mut u8;

        unsafe { (page_base.add(offset_in_page), 4096 - offset_in_page) }
    }
}
