use acpi::{AcpiTables, rsdp::Rsdp};
use conquer_once::spin::OnceCell;

use crate::acpi::handler::ACPIHandler;

pub mod handler;

pub static ACPI_TABLE: OnceCell<AcpiTables<ACPIHandler>> = OnceCell::uninit();

pub fn init(rsdp_addr: u64) {
    log::debug!("acpi: init start");
    let handler = ACPIHandler {};
    ACPI_TABLE
        .try_get_or_init(|| unsafe {
            AcpiTables::from_rsdp(handler, rsdp_addr as usize)
                .expect("Failed to parse ACPI Table from RSDT")
        })
        .expect("Failed to initalize ACPI Table");
    log::debug!("acpi: init done");
}
