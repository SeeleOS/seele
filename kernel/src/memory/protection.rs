bitflags::bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct Protection: u64 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXEC = 1 << 2;
    }
}
