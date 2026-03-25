use x86_64::{VirtAddr, structures::tss::TaskStateSegment};

pub const DOUBLE_FAULT_IST_LOCATION: u16 = 0;
pub const PAGE_FAULT_IST_LOCATION: u16 = 1;
pub const GP_IST_LOCATION: u16 = 2;

// a TSS is used to store the interrupt_stack_table (IST) and other stuff
pub static mut TSS: TaskStateSegment = TaskStateSegment::new();

pub fn init() {
    log::debug!("tss: init start");
    let mut tss = TaskStateSegment::new();

    tss.interrupt_stack_table[DOUBLE_FAULT_IST_LOCATION as usize] = {
        const STACK_SIZE: usize = 4096 * 5;

        // doing some dark magic wizardy to create a stack by declaring a
        // static mut array
        static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

        // load the stack created with the dark magic wizardy above with a refrence to the
        // stack or something idk this shit is too dark magic to me
        let stack_start = VirtAddr::from_ptr(&raw const STACK);
        stack_start + STACK_SIZE as u64
    };

    tss.interrupt_stack_table[PAGE_FAULT_IST_LOCATION as usize] = {
        const STACK_SIZE: usize = 4096 * 5;

        // doing some dark magic wizardy to create a stack by declaring a
        // static mut array
        static mut STACKY: [u8; STACK_SIZE] = [0; STACK_SIZE];

        // load the stack created with the dark magic wizardy above with a refrence to the
        // stack or something idk this shit is too dark magic to me
        let stack_start = VirtAddr::from_ptr(&raw const STACKY);
        stack_start + STACK_SIZE as u64
    };

    tss.interrupt_stack_table[GP_IST_LOCATION as usize] = {
        const STACK_SIZE: usize = 4096 * 5;

        // doing some dark magic wizardy to create a stack by declaring a
        // static mut array
        static mut STACKZS: [u8; STACK_SIZE] = [0; STACK_SIZE];

        // load the stack created with the dark magic wizardy above with a refrence to the
        // stack or something idk this shit is too dark magic to me
        let stack_start = VirtAddr::from_ptr(&raw const STACKZS);
        stack_start + STACK_SIZE as u64
    };

    unsafe {
        TSS = tss;
    }
    log::debug!("tss: init done");
}

pub fn get_ref() -> &'static TaskStateSegment {
    unsafe { &*(&raw const TSS) }
}
