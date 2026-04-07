use alloc::{string::String, vec, vec::Vec};
use xmas_elf::{ElfFile, header::HeaderPt2, program::ProgramHeader};

use crate::filesystem::{errors::FSError, object::FileLikeObject};

fn header_bytes_len(pt2: HeaderPt2<'_>) -> usize {
    pt2.ph_offset() as usize + pt2.ph_entry_size() as usize * pt2.ph_count() as usize
}

pub fn read_elf_header(file: &FileLikeObject) -> Result<Vec<u8>, FSError> {
    let mut header_prefix = vec![0u8; 128];
    let read = file.read_at(&mut header_prefix, 0)?;
    header_prefix.truncate(read);

    let elf = ElfFile::new(&header_prefix).map_err(|_| FSError::Other)?;
    let total = header_bytes_len(elf.header.pt2);
    let mut bytes = vec![0u8; total];
    file.read_exact_at(&mut bytes, 0)?;
    Ok(bytes)
}

pub fn read_interp(file: &FileLikeObject, ph: ProgramHeader<'_>) -> Result<String, FSError> {
    let mut bytes = vec![0u8; ph.file_size() as usize];
    file.read_exact_at(&mut bytes, ph.offset())?;
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let path = core::str::from_utf8(&bytes[..end]).map_err(|_| FSError::Other)?;
    Ok(path.into())
}
