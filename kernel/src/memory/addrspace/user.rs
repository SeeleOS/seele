use core::ptr::copy_nonoverlapping;

use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTableFlags, Size4KiB, Translate, mapper::TranslateResult},
};

use crate::{
    memory::addrspace::{AddrSpace, cow::COW_FLAG, mem_area::Data},
    systemcall::utils::{SyscallError, SyscallResult},
};

impl AddrSpace {
    fn ensure_user_page_readable(&mut self, addr: VirtAddr) -> bool {
        match self.page_table.inner.translate(addr) {
            TranslateResult::Mapped { .. } => true,
            _ => match self.get_area(addr).cloned() {
                Some(area) if area.lazy => {
                    let page = Page::<Size4KiB>::containing_address(addr);
                    let is_file_backed = matches!(area.data, Data::File { .. });
                    if is_file_backed {
                        self.apply_page_cluster(page, area, Self::file_lazy_cluster_pages());
                    } else {
                        self.apply_page(page, area);
                    }
                    true
                }
                _ => false,
            },
        }
    }

    fn ensure_user_page_writable(&mut self, addr: VirtAddr) -> bool {
        match self.page_table.inner.translate(addr) {
            TranslateResult::Mapped { flags, .. } => {
                if flags.contains(COW_FLAG) {
                    self.replace_cow_page(addr);
                    true
                } else {
                    flags.contains(PageTableFlags::WRITABLE)
                }
            }
            _ => match self.get_area(addr).cloned() {
                Some(area) if area.lazy && area.flags.contains(PageTableFlags::WRITABLE) => {
                    let page = Page::<Size4KiB>::containing_address(addr);
                    let is_file_backed = matches!(area.data, Data::File { .. });
                    if is_file_backed {
                        self.apply_page_cluster(page, area, Self::file_lazy_cluster_pages());
                    } else {
                        self.apply_page(page, area);
                    }
                    true
                }
                _ => false,
            },
        }
    }

    fn write_bytes(&mut self, mut addr: u64, mut src: &[u8]) -> SyscallResult<()> {
        while !src.is_empty() {
            let virt = VirtAddr::new(addr);
            if !self.ensure_user_page_writable(virt) {
                return Err(SyscallError::BadAddress);
            }

            let Some(phys) = self.translate_addr(virt) else {
                return Err(SyscallError::BadAddress);
            };

            let page_offset = (virt.as_u64() & 0xfff) as usize;
            let chunk_len = src.len().min(4096 - page_offset);
            let dst = crate::memory::utils::apply_offset(phys.as_u64()) as *mut u8;

            unsafe {
                copy_nonoverlapping(src.as_ptr(), dst, chunk_len);
            }

            addr += chunk_len as u64;
            src = &src[chunk_len..];
        }

        Ok(())
    }

    fn read_bytes(&mut self, mut addr: u64, mut dst: &mut [u8]) -> SyscallResult<()> {
        while !dst.is_empty() {
            let virt = VirtAddr::new(addr);
            if !self.ensure_user_page_readable(virt) {
                return Err(SyscallError::BadAddress);
            }

            let Some(phys) = self.translate_addr(virt) else {
                return Err(SyscallError::BadAddress);
            };

            let page_offset = (virt.as_u64() & 0xfff) as usize;
            let chunk_len = dst.len().min(4096 - page_offset);
            let src = crate::memory::utils::apply_offset(phys.as_u64()) as *const u8;

            unsafe {
                copy_nonoverlapping(src, dst.as_mut_ptr(), chunk_len);
            }

            addr += chunk_len as u64;
            dst = &mut dst[chunk_len..];
        }

        Ok(())
    }

    pub fn write<T: ?Sized, U>(&mut self, ptr: *mut U, value: &T) -> SyscallResult<()> {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(value).cast::<u8>(),
                core::mem::size_of_val(value),
            )
        };
        self.write_bytes(ptr as u64, bytes)
    }

    pub fn read<T: Copy>(&mut self, ptr: *const T) -> SyscallResult<T> {
        let mut value = MaybeUninit::<T>::uninit();
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                value.as_mut_ptr().cast::<u8>(),
                core::mem::size_of::<T>(),
            )
        };
        self.read_bytes(ptr as u64, bytes)?;
        Ok(unsafe { value.assume_init() })
    }
}
use core::mem::MaybeUninit;
