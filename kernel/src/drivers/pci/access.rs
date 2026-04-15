use virtio_drivers::transport::pci::bus::{ConfigurationAccess, DeviceFunction};
use x86_64::instructions::port::Port;

#[derive(Clone, Copy, Debug, Default)]
pub struct PciConfigPorts;

impl PciConfigPorts {
    const CONFIG_ADDRESS_PORT: u16 = 0x0cf8;
    const CONFIG_DATA_PORT: u16 = 0x0cfc;

    fn address(device_function: DeviceFunction, register_offset: u8) -> u32 {
        debug_assert_eq!(register_offset & 0x3, 0);

        0x8000_0000
            | ((device_function.bus as u32) << 16)
            | ((device_function.device as u32) << 11)
            | ((device_function.function as u32) << 8)
            | ((register_offset as u32) & 0xfc)
    }
}

impl ConfigurationAccess for PciConfigPorts {
    fn read_word(&self, device_function: DeviceFunction, register_offset: u8) -> u32 {
        let address = Self::address(device_function, register_offset);
        let mut address_port = Port::<u32>::new(Self::CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(Self::CONFIG_DATA_PORT);

        unsafe {
            address_port.write(address);
            data_port.read()
        }
    }

    fn write_word(&mut self, device_function: DeviceFunction, register_offset: u8, data: u32) {
        let address = Self::address(device_function, register_offset);
        let mut address_port = Port::<u32>::new(Self::CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(Self::CONFIG_DATA_PORT);

        unsafe {
            address_port.write(address);
            data_port.write(data);
        }
    }

    unsafe fn unsafe_clone(&self) -> Self {
        *self
    }
}
