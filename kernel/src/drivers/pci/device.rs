use alloc::vec::Vec;

use virtio_drivers::transport::pci::bus::{DeviceFunction, DeviceFunctionInfo, PciRoot};

use crate::drivers::pci::access::PciConfigPorts;

#[derive(Clone, Debug)]
pub struct PciDeviceRecord {
    pub function: DeviceFunction,
    pub info: DeviceFunctionInfo,
}

pub fn enumerate_devices() -> Vec<PciDeviceRecord> {
    let root = PciRoot::new(PciConfigPorts);
    let mut devices = Vec::new();

    for bus in 0..=u8::MAX {
        for (function, info) in root.enumerate_bus(bus) {
            log::debug!(
                "pci: {:02x}:{:02x}.{} vendor={:#06x} device={:#06x} class={:#04x} subclass={:#04x}",
                function.bus,
                function.device,
                function.function,
                info.vendor_id,
                info.device_id,
                info.class,
                info.subclass,
            );
            devices.push(PciDeviceRecord { function, info });
        }
    }

    devices
}
