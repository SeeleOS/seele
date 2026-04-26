pub mod dma;
pub mod net;
pub mod pci;
pub mod virtio;

pub fn init_early() {
    virtio::block::init();
}

pub fn init_late() {
    net::init();
}
