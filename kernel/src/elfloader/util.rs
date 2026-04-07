pub fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

pub fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}
