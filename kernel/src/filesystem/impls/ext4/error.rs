use ext4plus::{Ext4Write, error::Ext4Error};

use crate::filesystem::{block_device::BlockDeviceError, errors::FSError};

impl From<Ext4Error> for FSError {
    fn from(value: Ext4Error) -> Self {
        match value {
            Ext4Error::NotFound => Self::NotFound,
            Ext4Error::IsADirectory => Self::NotAFile,
            Ext4Error::Readonly => Self::Readonly,
            Ext4Error::NotADirectory => Self::NotADirectory,
            Ext4Error::Io(_) => Self::StorageDeviceError(BlockDeviceError::Other),
            _ => Self::Other,
        }
    }
}
