pub mod e1000;

use crate::drivers::pci::{PciDeviceRecord, enumerate_devices};

pub trait PciNetDriver: Sync {
    fn name(&self) -> &'static str;
    fn matches(&self, record: &PciDeviceRecord) -> bool;
    fn probe(&self, record: &PciDeviceRecord);
}

static DRIVERS: &[&dyn PciNetDriver] = &[&e1000::DRIVER];

pub fn init() {
    for record in enumerate_devices() {
        for driver in DRIVERS {
            if !driver.matches(&record) {
                continue;
            }

            log::info!(
                "drivers/net: {} matched {:02x}:{:02x}.{} vendor={:#06x} device={:#06x}",
                driver.name(),
                record.function.bus,
                record.function.device,
                record.function.function,
                record.info.vendor_id,
                record.info.device_id,
            );
            driver.probe(&record);
        }
    }
}
