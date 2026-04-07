use alloc::string::String;

#[derive(Clone, Debug)]
pub struct ElfInfo {
    pub entry_point: u64,
    pub program_header_table: u64,
    pub program_header_count: u16,
    pub program_header_entry_size: u16,
    pub interpreter: Option<String>,
    pub load_base: u64,
}
