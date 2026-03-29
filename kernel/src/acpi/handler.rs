use core::ptr::NonNull;

use acpi::{Handler, PhysicalMapping};
use x86_64::instructions::port::Port;

use crate::{
    memory::utils::apply_offset, misc::time::Time, read_addr, read_port, write_addr, write_port,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ACPIHandler;

impl Handler for ACPIHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        // I doesnt need to manually map stuff because bootlaoder already
        // mapped the memory :skull:
        PhysicalMapping {
            physical_start: physical_address,
            mapped_length: size,
            handler: self.clone(),
            region_length: size,
            virtual_start: NonNull::new(apply_offset(physical_address as u64) as *mut T).unwrap(),
        }
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}

    fn read_u8(&self, address: usize) -> u8 {
        unsafe { read_addr!(address, u8) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        unsafe { read_addr!(address, u16) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        unsafe { read_addr!(address, u32) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        unsafe { read_addr!(address, u64) }
    }

    fn read_io_u8(&self, port: u16) -> u8 {
        unsafe { read_port!(port) }
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        unsafe { read_port!(port) }
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        unsafe { read_port!(port) }
    }

    fn write_u8(&self, address: usize, value: u8) {
        unsafe { write_addr!(address, u8, value) }
    }

    fn write_u16(&self, address: usize, value: u16) {
        unsafe { write_addr!(address, u16, value) }
    }

    fn write_u32(&self, address: usize, value: u32) {
        unsafe { write_addr!(address, u32, value) }
    }

    fn write_u64(&self, address: usize, value: u64) {
        unsafe { write_addr!(address, u64, value) }
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        unsafe { write_port!(port, value) }
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        unsafe { write_port!(port, value) }
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        unsafe { write_port!(port, value) }
    }

    fn write_pci_u8(&self, _address: acpi::PciAddress, _offset: u16, _value: u8) {
        unimplemented!()
    }

    fn write_pci_u16(&self, _address: acpi::PciAddress, _offset: u16, _value: u16) {
        unimplemented!()
    }

    fn write_pci_u32(&self, _address: acpi::PciAddress, _offset: u16, _value: u32) {
        unimplemented!()
    }

    fn read_pci_u8(&self, _address: acpi::PciAddress, _offset: u16) -> u8 {
        unimplemented!()
    }

    fn read_pci_u16(&self, _address: acpi::PciAddress, _offset: u16) -> u16 {
        unimplemented!()
    }

    fn read_pci_u32(&self, _address: acpi::PciAddress, _offset: u16) -> u32 {
        unimplemented!()
    }

    fn nanos_since_boot(&self) -> u64 {
        Time::since_boot().as_nanoseconds()
    }

    fn stall(&self, _microseconds: u64) {
        unimplemented!()
    }

    fn sleep(&self, _milliseconds: u64) {
        unimplemented!()
    }

    fn create_mutex(&self) -> acpi::Handle {
        unimplemented!()
    }

    fn release(&self, _mutex: acpi::Handle) {
        unimplemented!()
    }

    fn acquire(&self, _mutex: acpi::Handle, _timeout: u16) -> Result<(), acpi::aml::AmlError> {
        unimplemented!()
    }
}
