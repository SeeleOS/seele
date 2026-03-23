use ext4plus::{Ext4Write, error::Ext4Error};

use crate::filesystem::errors::FSError;

impl From<Ext4Error> for FSError {
    fn from(value: Ext4Error) -> Self {
        match value {
            Ext4Error::NotFound => Self::NotFound,
            Ext4Error::IsADirectory => Self::NotAFile,
            Ext4Error::Readonly => Self::Readonly,
            Ext4Error::NotADirectory => Self::NotADirectory,
            _ => Self::Other,
        }
    }
}
