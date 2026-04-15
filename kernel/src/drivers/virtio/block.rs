use alloc::sync::Arc;
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
    filesystem::block_device::{BlockDevice, BlockDeviceError, BlockDeviceResult},
};

use virtio_drivers::transport::pci::bus::{Command, PciRoot};

static ROOT_DEVICE: OnceCell<Arc<dyn BlockDevice>> = OnceCell::uninit();

pub fn init() {
    let mut selected = None;

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

        if is_ext4_candidate(device.as_ref()) {
            let dyn_device: Arc<dyn BlockDevice> = device;
            let _ = ROOT_DEVICE.get_or_init(|| dyn_device.clone());
            selected = Some(dyn_device);
            break;
        }
    }

    if selected.is_none() {
        log::warn!("virtio-blk: no ext4-capable virtio block device selected");
    }
}

pub fn root_device() -> Option<Arc<dyn BlockDevice>> {
    ROOT_DEVICE.get().cloned()
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
    inner: Mutex<VirtIOBlk<KernelHal, PciTransport>>,
    capacity: usize,
    readonly: bool,
}

impl VirtioBlockDevice {
    fn new(
        function: virtio_drivers::transport::pci::bus::DeviceFunction,
    ) -> Option<Self> {
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
            inner: Mutex::new(block),
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
        if start + (buffer.len() / SECTOR_SIZE) > self.capacity {
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
        if start + (buffer.len() / SECTOR_SIZE) > self.capacity {
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
