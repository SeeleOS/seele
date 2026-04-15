pub mod pci;
pub mod virtio;

pub fn init() {
    virtio::block::init();
}
