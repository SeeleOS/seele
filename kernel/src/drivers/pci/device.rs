use alloc::vec::Vec;
use conquer_once::spin::OnceCell;
use pci_types::PciAddress;

use virtio_drivers::transport::pci::bus::{DeviceFunction, DeviceFunctionInfo, HeaderType, PciRoot};

use crate::drivers::pci::access::PciConfigPorts;

static DEVICES: OnceCell<Vec<PciDeviceRecord>> = OnceCell::uninit();

#[derive(Clone, Debug)]
pub struct PciDeviceRecord {
    pub address: PciAddress,
    pub function: DeviceFunction,
    pub info: DeviceFunctionInfo,
}

pub fn enumerate_devices() -> &'static [PciDeviceRecord] {
    DEVICES.get_or_init(discover_devices).as_slice()
}

fn discover_devices() -> Vec<PciDeviceRecord> {
    let root = PciRoot::new(PciConfigPorts);
    let mut devices = Vec::new();
    let mut visited_buses = [false; 256];

    discover_bus(&root, 0, &mut visited_buses, &mut devices);

    devices
}

fn discover_bus(
    root: &PciRoot<PciConfigPorts>,
    bus: u8,
    visited_buses: &mut [bool; 256],
    devices: &mut Vec<PciDeviceRecord>,
) {
    if visited_buses[bus as usize] {
        return;
    }
    visited_buses[bus as usize] = true;

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
        devices.push(PciDeviceRecord {
            address: PciConfigPorts::pci_address(function),
            function,
            info: info.clone(),
        });

        if info.header_type == HeaderType::PciPciBridge {
            let secondary_bus = PciConfigPorts.read_u8(function, 0x19);
            if secondary_bus != 0 {
                discover_bus(root, secondary_bus, visited_buses, devices);
            }
        }
    }
}
