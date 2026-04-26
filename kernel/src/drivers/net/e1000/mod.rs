use alloc::{sync::Arc, vec, vec::Vec};
use core::{mem::size_of, ptr};

use spin::Mutex;
use virtio_drivers::transport::pci::bus::{BarInfo, Command, PciRoot};

use crate::{
    drivers::{
        dma::DmaRegion,
        net::PciNetDriver,
        pci::{PciConfigPorts, PciDeviceRecord},
    },
    memory::mmio::map_mmio,
    net::{self, NetError, NetResult, NetworkDevice},
};

pub struct E1000Driver;

pub static DRIVER: E1000Driver = E1000Driver;

const VENDOR_ID_INTEL: u16 = 0x8086;
const DEVICE_ID_82540EM: u16 = 0x100e;
const DEVICE_ID_82545EM: u16 = 0x100f;

const REG_CTRL: u32 = 0x0000;
const REG_IMC: u32 = 0x00d8;
const REG_RCTL: u32 = 0x0100;
const REG_TCTL: u32 = 0x0400;
const REG_TIPG: u32 = 0x0410;
const REG_RDBAL: u32 = 0x2800;
const REG_RDBAH: u32 = 0x2804;
const REG_RDLEN: u32 = 0x2808;
const REG_RDH: u32 = 0x2810;
const REG_RDT: u32 = 0x2818;
const REG_TDBAL: u32 = 0x3800;
const REG_TDBAH: u32 = 0x3804;
const REG_TDLEN: u32 = 0x3808;
const REG_TDH: u32 = 0x3810;
const REG_TDT: u32 = 0x3818;
const REG_RAL0: u32 = 0x5400;
const REG_RAH0: u32 = 0x5404;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_ASDE: u32 = 1 << 5;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_RTLC: u32 = 1 << 24;

const RX_DESC_STATUS_DD: u8 = 1 << 0;
const TX_DESC_CMD_EOP: u8 = 1 << 0;
const TX_DESC_CMD_IFCS: u8 = 1 << 1;
const TX_DESC_CMD_RS: u8 = 1 << 3;
const TX_DESC_STATUS_DD: u8 = 1 << 0;

const RX_DESC_COUNT: usize = 32;
const TX_DESC_COUNT: usize = 32;
const RX_BUFFER_SIZE: usize = 2048;
const TX_BUFFER_SIZE: usize = 2048;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RxDescriptor {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TxDescriptor {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[derive(Debug)]
struct E1000State {
    rx_next: usize,
    tx_next: usize,
}

#[derive(Debug)]
struct E1000Device {
    mmio_base: u64,
    mac: [u8; 6],
    state: Mutex<E1000State>,
    rx_ring: DmaRegion,
    tx_ring: DmaRegion,
    rx_buffers: Vec<DmaRegion>,
    tx_buffers: Vec<DmaRegion>,
}

impl PciNetDriver for E1000Driver {
    fn name(&self) -> &'static str {
        "e1000"
    }

    fn matches(&self, record: &PciDeviceRecord) -> bool {
        record.info.vendor_id == VENDOR_ID_INTEL
            && matches!(record.info.device_id, DEVICE_ID_82540EM | DEVICE_ID_82545EM)
    }

    fn probe(&self, record: &PciDeviceRecord) {
        let Some(device) = E1000Device::probe(record) else {
            return;
        };
        let device: Arc<dyn NetworkDevice> = device;
        net::register_device(device);
    }
}

impl E1000Device {
    fn probe(record: &PciDeviceRecord) -> Option<Arc<Self>> {
        let mut root = PciRoot::new(PciConfigPorts);
        let (_, command) = root.get_status_command(record.function);
        let desired = command | Command::BUS_MASTER | Command::MEMORY_SPACE;
        if desired != command {
            root.set_command(record.function, desired);
        }

        let bars = match root.bars(record.function) {
            Ok(bars) => bars,
            Err(err) => {
                log::warn!("e1000: failed to read BARs: {err}");
                return None;
            }
        };
        let Some((bar_addr, bar_size)) = bars.into_iter().flatten().find_map(|bar| match bar {
            BarInfo::Memory { .. } => bar.memory_address_size(),
            BarInfo::IO { .. } => None,
        }) else {
            log::warn!("e1000: no MMIO BAR found");
            return None;
        };

        let mmio_base = map_mmio(bar_addr, bar_size as usize);
        let mut device = Self::new(mmio_base)?;
        device.init_hw();
        log::info!(
            "e1000: ready on {:02x}:{:02x}.{} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            record.function.bus,
            record.function.device,
            record.function.function,
            device.mac[0],
            device.mac[1],
            device.mac[2],
            device.mac[3],
            device.mac[4],
            device.mac[5],
        );
        Some(Arc::new(device))
    }

    fn new(mmio_base: u64) -> Option<Self> {
        let rx_ring = DmaRegion::new(RX_DESC_COUNT * size_of::<RxDescriptor>())?;
        let tx_ring = DmaRegion::new(TX_DESC_COUNT * size_of::<TxDescriptor>())?;
        let mut rx_buffers = Vec::with_capacity(RX_DESC_COUNT);
        let mut tx_buffers = Vec::with_capacity(TX_DESC_COUNT);
        for _ in 0..RX_DESC_COUNT {
            rx_buffers.push(DmaRegion::new(RX_BUFFER_SIZE)?);
        }
        for _ in 0..TX_DESC_COUNT {
            tx_buffers.push(DmaRegion::new(TX_BUFFER_SIZE)?);
        }

        let mut device = Self {
            mmio_base,
            mac: [0; 6],
            state: Mutex::new(E1000State {
                rx_next: 0,
                tx_next: 0,
            }),
            rx_ring,
            tx_ring,
            rx_buffers,
            tx_buffers,
        };
        device.mac = device.read_mac();
        Some(device)
    }

    fn init_hw(&mut self) {
        self.write_reg(REG_IMC, u32::MAX);
        self.write_reg(REG_CTRL, self.read_reg(REG_CTRL) | CTRL_SLU | CTRL_ASDE);
        self.setup_rx();
        self.setup_tx();
    }

    fn setup_rx(&mut self) {
        let buffer_addrs: Vec<u64> = self.rx_buffers.iter().map(DmaRegion::phys_addr).collect();
        let descriptors = self.rx_descriptors_mut();
        for (index, desc) in descriptors.iter_mut().enumerate() {
            *desc = RxDescriptor {
                addr: buffer_addrs[index],
                ..RxDescriptor::default()
            };
        }

        self.write_reg(REG_RDBAL, self.rx_ring.phys_addr() as u32);
        self.write_reg(REG_RDBAH, (self.rx_ring.phys_addr() >> 32) as u32);
        self.write_reg(
            REG_RDLEN,
            (RX_DESC_COUNT * size_of::<RxDescriptor>()) as u32,
        );
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (RX_DESC_COUNT - 1) as u32);
        self.write_reg(REG_RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
    }

    fn setup_tx(&mut self) {
        let buffer_addrs: Vec<u64> = self.tx_buffers.iter().map(DmaRegion::phys_addr).collect();
        let descriptors = self.tx_descriptors_mut();
        for (index, desc) in descriptors.iter_mut().enumerate() {
            *desc = TxDescriptor {
                addr: buffer_addrs[index],
                status: TX_DESC_STATUS_DD,
                ..TxDescriptor::default()
            };
        }

        self.write_reg(REG_TDBAL, self.tx_ring.phys_addr() as u32);
        self.write_reg(REG_TDBAH, (self.tx_ring.phys_addr() >> 32) as u32);
        self.write_reg(
            REG_TDLEN,
            (TX_DESC_COUNT * size_of::<TxDescriptor>()) as u32,
        );
        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.write_reg(
            REG_TCTL,
            TCTL_EN | TCTL_PSP | TCTL_RTLC | (0x0f << 4) | (0x40 << 12),
        );
        self.write_reg(REG_TIPG, 10 | (8 << 10) | (6 << 20));
    }

    fn read_mac(&self) -> [u8; 6] {
        let ral = self.read_reg(REG_RAL0);
        let rah = self.read_reg(REG_RAH0);
        [
            (ral & 0xff) as u8,
            ((ral >> 8) & 0xff) as u8,
            ((ral >> 16) & 0xff) as u8,
            ((ral >> 24) & 0xff) as u8,
            (rah & 0xff) as u8,
            ((rah >> 8) & 0xff) as u8,
        ]
    }

    fn read_reg(&self, offset: u32) -> u32 {
        unsafe { ptr::read_volatile((self.mmio_base + offset as u64) as *const u32) }
    }

    fn write_reg(&self, offset: u32, value: u32) {
        unsafe {
            ptr::write_volatile((self.mmio_base + offset as u64) as *mut u32, value);
        }
    }

    fn rx_descriptors_mut(&mut self) -> &mut [RxDescriptor] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.rx_ring.as_mut_ptr::<RxDescriptor>(),
                RX_DESC_COUNT,
            )
        }
    }

    fn tx_descriptors_mut(&mut self) -> &mut [TxDescriptor] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.tx_ring.as_mut_ptr::<TxDescriptor>(),
                TX_DESC_COUNT,
            )
        }
    }

    fn rx_descriptors(&self) -> &[RxDescriptor] {
        unsafe { core::slice::from_raw_parts(self.rx_ring.as_ptr::<RxDescriptor>(), RX_DESC_COUNT) }
    }

    fn tx_descriptors(&self) -> &[TxDescriptor] {
        unsafe { core::slice::from_raw_parts(self.tx_ring.as_ptr::<TxDescriptor>(), TX_DESC_COUNT) }
    }
}

impl NetworkDevice for E1000Device {
    fn name(&self) -> &'static str {
        "eth0"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn mtu(&self) -> usize {
        1500
    }

    fn receive(&self) -> Option<Vec<u8>> {
        let mut state = self.state.lock();
        let index = state.rx_next;
        let desc = &self.rx_descriptors()[index];
        if desc.status & RX_DESC_STATUS_DD == 0 || desc.length == 0 {
            return None;
        }

        let length = usize::from(desc.length);
        let mut frame = vec![0; length];
        frame.copy_from_slice(&self.rx_buffers[index].as_slice()[..length]);

        unsafe {
            let desc_ptr = self.rx_ring.as_mut_ptr::<RxDescriptor>().add(index);
            (*desc_ptr).status = 0;
            (*desc_ptr).length = 0;
        }
        self.write_reg(REG_RDT, index as u32);
        state.rx_next = (state.rx_next + 1) % RX_DESC_COUNT;

        Some(frame)
    }

    fn transmit(&self, frame: &[u8]) -> NetResult<()> {
        if frame.len() > TX_BUFFER_SIZE {
            return Err(NetError::InvalidArguments);
        }

        let mut state = self.state.lock();
        let index = state.tx_next;
        let desc = &self.tx_descriptors()[index];
        if desc.status & TX_DESC_STATUS_DD == 0 {
            return Err(NetError::TryAgain);
        }

        unsafe {
            ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.tx_buffers[index].as_ptr::<u8>() as *mut u8,
                frame.len(),
            );
        }

        unsafe {
            let desc_ptr = self.tx_ring.as_mut_ptr::<TxDescriptor>().add(index);
            (*desc_ptr).addr = self.tx_buffers[index].phys_addr();
            (*desc_ptr).length = frame.len() as u16;
            (*desc_ptr).cmd = TX_DESC_CMD_EOP | TX_DESC_CMD_IFCS | TX_DESC_CMD_RS;
            (*desc_ptr).status = 0;
        }

        state.tx_next = (state.tx_next + 1) % TX_DESC_COUNT;
        self.write_reg(REG_TDT, state.tx_next as u32);
        Ok(())
    }
}
