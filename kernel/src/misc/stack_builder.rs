use elfloader::LoadedElf;
use x86_64::VirtAddr;

use crate::misc::auxv::AuxType;

#[derive(Debug)]
pub struct StackBuilder {
    sp: VirtAddr,
    write_sp: *mut u8,
}

impl StackBuilder {
    pub fn new(sp: u64, write_sp: *mut u8) -> Self {
        Self {
            sp: VirtAddr::new(sp),
            write_sp,
        }
    }

    pub fn push_aux_entries(&mut self, file: &LoadedElf, interpreter_base: Option<u64>) {
        self.push_aux_entry(AuxType::Null, 0);
        self.push_aux_entry(AuxType::EntryPointAddress, file.entry_point());
        self.push_aux_entry(
            AuxType::ProgramHeaderAmount,
            file.program_header_count() as u64,
        );
        self.push_aux_entry(
            AuxType::ProgramHeaderEntrySize,
            file.program_header_entry_size() as u64,
        );
        self.push_aux_entry(AuxType::ProgramHeaderTable, file.program_header_table());
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
        unsafe { write_u64_and_sub(&mut self.write_sp, value) };
    }

    pub fn push_str(&mut self, s: &str) -> u64 {
        let bytes = s.as_bytes();
        let len_with_null = (bytes.len() + 1) as u64;

        self.sp -= len_with_null;
        self.write_sp = unsafe { self.write_sp.sub(len_with_null as usize) };

        let user_vaddr = self.sp;
        let kernel_vaddr = self.write_sp;

        unsafe {
            let slice = core::slice::from_raw_parts_mut(kernel_vaddr, len_with_null as usize);
            slice[..bytes.len()].copy_from_slice(bytes);
            slice[bytes.len()] = 0;
        }

        user_vaddr.as_u64()
    }

    pub fn push_struct<T: Copy>(&mut self, value: &T) -> u64 {
        let size = core::mem::size_of::<T>() as u64;

        self.sp -= size;
        self.write_sp = unsafe { self.write_sp.sub(size as usize) };

        unsafe {
            self.write_sp.cast::<T>().write_unaligned(*value);
        }

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
        unsafe {
            self.write_sp = self.write_sp.sub(bytes as usize);
            core::ptr::write_bytes(self.write_sp, 0, bytes as usize);
        }
    }
}

/// # Safety
/// Must provide valid pointer
unsafe fn write_u64_and_sub(ptr: &mut *mut u8, data: u64) {
    unsafe {
        *ptr = ptr.sub(8);
        ptr.cast::<u64>().write_unaligned(data);
    }
}
