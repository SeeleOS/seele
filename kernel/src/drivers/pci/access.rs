use pci_types::{ConfigRegionAccess, PciAddress};
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

    pub fn pci_address(device_function: DeviceFunction) -> PciAddress {
        PciAddress::new(
            0,
            device_function.bus,
            device_function.device,
            device_function.function,
        )
    }

    pub fn read_u32(&self, device_function: DeviceFunction, register_offset: u8) -> u32 {
        self.read_word(device_function, register_offset)
    }

    pub fn write_u32(&mut self, device_function: DeviceFunction, register_offset: u8, data: u32) {
        self.write_word(device_function, register_offset, data);
    }

    pub fn read_u16(&self, device_function: DeviceFunction, register_offset: u8) -> u16 {
        let aligned = register_offset & !0x3;
        let shift = u32::from((register_offset & 0x2) * 8);
        ((self.read_word(device_function, aligned) >> shift) & 0xffff) as u16
    }

    pub fn write_u16(&mut self, device_function: DeviceFunction, register_offset: u8, data: u16) {
        let aligned = register_offset & !0x3;
        let shift = u32::from((register_offset & 0x2) * 8);
        let mask = !(0xffff_u32 << shift);
        let value = (self.read_word(device_function, aligned) & mask) | ((data as u32) << shift);
        self.write_word(device_function, aligned, value);
    }

    pub fn read_u8(&self, device_function: DeviceFunction, register_offset: u8) -> u8 {
        let aligned = register_offset & !0x3;
        let shift = u32::from((register_offset & 0x3) * 8);
        ((self.read_word(device_function, aligned) >> shift) & 0xff) as u8
    }

    pub fn write_u8(&mut self, device_function: DeviceFunction, register_offset: u8, data: u8) {
        let aligned = register_offset & !0x3;
        let shift = u32::from((register_offset & 0x3) * 8);
        let mask = !(0xff_u32 << shift);
        let value = (self.read_word(device_function, aligned) & mask) | ((data as u32) << shift);
        self.write_word(device_function, aligned, value);
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

impl ConfigRegionAccess for PciConfigPorts {
    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        self.read_word(
            DeviceFunction {
                bus: address.bus(),
                device: address.device(),
                function: address.function(),
            },
            offset as u8,
        )
    }

    unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
        let mut clone = *self;
        clone.write_word(
            DeviceFunction {
                bus: address.bus(),
                device: address.device(),
                function: address.function(),
            },
            offset as u8,
            value,
        );
    }
}
