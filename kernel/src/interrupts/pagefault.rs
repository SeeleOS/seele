use x86_64::{
    registers::control::Cr2,
    structures::idt::{InterruptStackFrame, PageFaultErrorCode},
};

use crate::{
    multitasking::{MANAGER, process::manager::get_current_process},
    println,
};

pub extern "x86-interrupt" fn pagefault_handler(_: InterruptStackFrame, _: PageFaultErrorCode) {
    let addr = Cr2::read().unwrap();

    let process = get_current_process();
    let addrspace = &mut process.lock().addrspace;

    match addrspace.get_area(addr) {
        Some(area) => {
            addrspace.apply_region(*area);
        }
        None => {
            todo!()
        }
    }
}
