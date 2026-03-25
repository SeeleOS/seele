use lazy_static::lazy_static;
use x86_64::{
    instructions::tables::load_tss,
    registers::segmentation::{CS, DS, ES, SS, Segment},
    structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector},
};

use crate::tss::{self};

lazy_static! {
    pub static ref GDT: (GlobalDescriptorTable, GDTSelectors)= {
        let mut gdt = GlobalDescriptorTable::new();

        // a selector is just a fancy way of saying index. it stores the index and
        // other stuffs about the GDT entry
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());

        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code = gdt.append(Descriptor::user_code_segment());

        let tss_selector = gdt.append(Descriptor::tss_segment(tss::get_ref()));

        (gdt, GDTSelectors { kernel_code, kernel_data, tss_selector, user_data, user_code })
    };
}

pub struct GDTSelectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

pub fn init() {
    log::debug!("gdt: init start");
    GDT.0.load();

    unsafe {
        // updates the CS so that it knows the gdt stuff or
        // whatever have changed.
        CS::set_reg(GDT.1.kernel_code);
        // Ensure data segments use the kernel data selector.
        SS::set_reg(GDT.1.kernel_data);
        DS::set_reg(GDT.1.kernel_data);
        ES::set_reg(GDT.1.kernel_data);
        // load the tss from the gdt entry
        load_tss(GDT.1.tss_selector);
    }
    log::debug!("gdt: init done");
}
