use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc, vec::Vec};
use conquer_once::spin::OnceCell;
use spin::Mutex;
use virtio_drivers::{
    Error as VirtioError,
    device::blk::{SECTOR_SIZE, VirtIOBlk},
    transport::{
        DeviceType,
        pci::{PciTransport, virtio_device_type},
    },
};

use crate::{
    drivers::{
        pci::{PciConfigPorts, enumerate_devices},
        virtio::hal::KernelHal,
    },
    filesystem::{
        block_device::{BlockDevice, BlockDeviceError, BlockDeviceResult},
        info::LinuxStat,
    },
};

use virtio_drivers::transport::pci::bus::{Command, PciRoot};

static ROOT_DEVICE: OnceCell<Arc<dyn BlockDevice>> = OnceCell::uninit();
static DEVICES: Mutex<Vec<NamedBlockDevice>> = Mutex::new(Vec::new());
const VIRTIO_BLK_MAJOR: u64 = 252;
const VIRTIO_BLK_DISK_MINOR_STRIDE: u64 = 16;

#[derive(Clone)]
pub struct NamedBlockDevice {
    pub name: String,
    pub minor: u64,
    pub device: Arc<dyn BlockDevice>,
}

impl NamedBlockDevice {
    pub fn rdev(&self) -> u64 {
        LinuxStat::linux_makedev(VIRTIO_BLK_MAJOR, self.minor)
    }
}

pub fn init() {
    let mut selected = None;
    let mut index = 0usize;

    for record in enumerate_devices() {
        if record.info.device_id < 0x1040 {
            log::debug!(
                "virtio-blk: skipping legacy/transitional PCI function {:02x}:{:02x}.{} device={:#06x}",
                record.function.bus,
                record.function.device,
                record.function.function,
                record.info.device_id,
            );
            continue;
        }

        let Some(device_type) = virtio_device_type(&record.info) else {
            continue;
        };

        if device_type != DeviceType::Block {
            continue;
        }

        log::debug!(
            "virtio-blk: found PCI function {:02x}:{:02x}.{} device={:#06x}",
            record.function.bus,
            record.function.device,
            record.function.function,
            record.info.device_id,
        );

        let Some(device) = VirtioBlockDevice::new(record.function).map(Arc::new) else {
            continue;
        };

        log::debug!(
            "virtio-blk: capacity={} sectors readonly={}",
            device.total_blocks(),
            device.readonly,
        );

        let name = virtio_disk_name(index);
        let minor = index as u64 * VIRTIO_BLK_DISK_MINOR_STRIDE;
        index += 1;

        let dyn_device: Arc<dyn BlockDevice> = device;
        DEVICES.lock().push(NamedBlockDevice {
            name,
            minor,
            device: dyn_device.clone(),
        });

        if is_ext4_candidate(dyn_device.as_ref()) {
            let _ = ROOT_DEVICE.get_or_init(|| dyn_device.clone());
            selected = Some(dyn_device);
        }
    }

    if selected.is_none() {
        log::warn!("virtio-blk: no ext4-capable virtio block device selected");
    }
}

pub fn root_device() -> Option<Arc<dyn BlockDevice>> {
    ROOT_DEVICE.get().cloned()
}

pub fn named_device(name: &str) -> Option<NamedBlockDevice> {
    DEVICES
        .lock()
        .iter()
        .find(|device| device.name == name)
        .cloned()
}

pub fn list_devices() -> Vec<NamedBlockDevice> {
    DEVICES.lock().clone()
}

fn virtio_disk_name(index: usize) -> String {
    const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    let letter = LETTERS.get(index).copied().unwrap_or(b'z');
    alloc::format!("vd{}", letter as char)
}

fn is_ext4_candidate(device: &dyn BlockDevice) -> bool {
    log::debug!("virtio-blk: probing ext4 superblock");
    let mut magic = [0u8; 2];
    if device.read_by_bytes(1024 + 56, &mut magic).is_err() {
        log::warn!("virtio-blk: failed to read ext4 superblock");
        return false;
    }

    let is_ext4 = u16::from_le_bytes(magic) == 0xef53;
    log::debug!("virtio-blk: ext4 superblock match={}", is_ext4);
    is_ext4
}

struct VirtioBlockDevice {
    inner: Mut<VirtIOBlk<KernelHal, PciTransport>>,
    capacity: usize,
    readonly: bool,
}

impl VirtioBlockDevice {
    fn new(function: virtio_drivers::transport::pci::bus::DeviceFunction) -> Option<Self> {
        let mut root = PciRoot::new(PciConfigPorts);
        let (_, command) = root.get_status_command(function);
        let desired = command | Command::BUS_MASTER | Command::MEMORY_SPACE;
        if desired != command {
            root.set_command(function, desired);
        }

        log::debug!(
            "virtio-blk: PCI command for {:02x}:{:02x}.{} = {:?}",
            function.bus,
            function.device,
            function.function,
            desired,
        );

        match root.bars(function) {
            Ok(bars) => {
                for (index, bar) in bars.into_iter().enumerate() {
                    if let Some(bar) = bar {
                        log::debug!("virtio-blk: BAR{index} = {bar}");
                    }
                }
            }
            Err(err) => {
                log::warn!("virtio-blk: failed to read BARs: {err}");
            }
        }

        log::debug!("virtio-blk: building PCI transport");

        let transport = match PciTransport::new::<KernelHal, _>(&mut root, function) {
            Ok(transport) => transport,
            Err(err) => {
                log::warn!("virtio-blk: failed to init PCI transport: {err}");
                return None;
            }
        };

        log::debug!("virtio-blk: PCI transport ready, building block queue");

        let block = match VirtIOBlk::<KernelHal, _>::new(transport) {
            Ok(block) => block,
            Err(err) => {
                log::warn!("virtio-blk: failed to init block device: {:?}", err);
                return None;
            }
        };

        log::debug!("virtio-blk: block queue ready");

        let capacity = block.capacity() as usize;
        let readonly = block.readonly();

        Some(Self {
            inner: Mut::new(block),
            capacity,
            readonly,
        })
    }

    fn map_error(err: VirtioError) -> BlockDeviceError {
        match err {
            VirtioError::QueueFull
            | VirtioError::NotReady
            | VirtioError::WrongToken
            | VirtioError::InvalidParam
            | VirtioError::DmaError
            | VirtioError::IoError
            | VirtioError::Unsupported
            | VirtioError::ConfigSpaceMissing
            | VirtioError::ConfigSpaceTooSmall => BlockDeviceError::Other,
            _ => BlockDeviceError::Other,
        }
    }
}

impl BlockDevice for VirtioBlockDevice {
    fn total_blocks(&self) -> usize {
        self.capacity
    }

    fn block_size(&self) -> usize {
        SECTOR_SIZE
    }

    fn read_single_block(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        if buffer.len() < SECTOR_SIZE {
            return Err(BlockDeviceError::BufferTooSmall);
        }
        if id >= self.capacity {
            return Err(BlockDeviceError::OutOfBounds);
        }

        self.inner
            .lock()
            .read_blocks(id, &mut buffer[..SECTOR_SIZE])
            .map_err(Self::map_error)?;

        Ok(SECTOR_SIZE)
    }

    fn read_blocks(&self, start: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        if !buffer.len().is_multiple_of(SECTOR_SIZE) {
            return Err(BlockDeviceError::BufferTooSmall);
        }
        let blocks = buffer.len() / SECTOR_SIZE;
        if start
            .checked_add(blocks)
            .is_none_or(|end| end > self.capacity)
        {
            return Err(BlockDeviceError::OutOfBounds);
        }

        self.inner
            .lock()
            .read_blocks(start, buffer)
            .map_err(Self::map_error)?;

        Ok(buffer.len())
    }

    fn write_single_block(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult {
        if self.readonly {
            return Err(BlockDeviceError::Readonly);
        }
        if buffer.len() < SECTOR_SIZE {
            return Err(BlockDeviceError::BufferTooSmall);
        }
        if id >= self.capacity {
            return Err(BlockDeviceError::OutOfBounds);
        }

        let mut inner = self.inner.lock();
        inner
            .write_blocks(id, &buffer[..SECTOR_SIZE])
            .map_err(Self::map_error)?;

        Ok(SECTOR_SIZE)
    }

    fn write_blocks(&self, start: usize, buffer: &[u8]) -> BlockDeviceResult {
        if self.readonly {
            return Err(BlockDeviceError::Readonly);
        }
        if !buffer.len().is_multiple_of(SECTOR_SIZE) {
            return Err(BlockDeviceError::BufferTooSmall);
        }
        let blocks = buffer.len() / SECTOR_SIZE;
        if start
            .checked_add(blocks)
            .is_none_or(|end| end > self.capacity)
        {
            return Err(BlockDeviceError::OutOfBounds);
        }

        self.inner
            .lock()
            .write_blocks(start, buffer)
            .map_err(Self::map_error)?;

        Ok(buffer.len())
    }

    fn flush(&self) -> Result<(), BlockDeviceError> {
        self.inner.lock().flush().map_err(Self::map_error)
    }
}
