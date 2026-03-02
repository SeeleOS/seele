use alloc::{string::String, sync::Arc, vec::Vec};
use fatfs::{IoBase, Read, ReadWriteSeek, Seek, Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::BlockDeviceError,
    errors::FSError,
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskReader},
    vfs_traits::{
        Directory, DirectoryContentInfo, DirectoryContentType, File, FileLike, FileSystem,
    },
};

pub struct FAT32(fatfs::FileSystem<Fat32RamDiskReader>);
pub struct Fat32RamDiskReader(RamDiskReader);

impl IoBase for Fat32RamDiskReader {
    type Error = BlockDeviceError;
}

impl Write for Fat32RamDiskReader {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Read for Fat32RamDiskReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf)
    }
}

impl Seek for Fat32RamDiskReader {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        self.0.seek(SeekFrom::from(pos))
    }
}

impl From<fatfs::SeekFrom> for SeekFrom {
    fn from(value: fatfs::SeekFrom) -> Self {
        match value {
            fatfs::SeekFrom::Start(val) => Self::Start(val),
            fatfs::SeekFrom::End(val) => Self::End(val),
            fatfs::SeekFrom::Current(val) => Self::Current(val),
        }
    }
}

#[derive(Debug)]
pub struct FAT32File {
    name: String,
    inner: fatfs::File<
        'static,
        Fat32RamDiskReader,
        fatfs::DefaultTimeProvider,
        fatfs::LossyOemCpConverter,
    >,
}

impl FAT32File {
    pub fn new(
        name: String,
        inner: fatfs::File<
            'static,
            Fat32RamDiskReader,
            fatfs::DefaultTimeProvider,
            fatfs::LossyOemCpConverter,
        >,
    ) -> Self {
        Self { name, inner }
    }
}

impl File for FAT32File {
    fn read(&self, buffer: &mut [u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.read(buffer).map_err(|_| FSError::NotFound)
    }

    fn write(&mut self, buffer: &[u8]) -> crate::filesystem::vfs::FSResult<usize> {
        self.inner.write(buffer).map_err(|_| FSError::NotFound)
    }

    fn name(&self) -> crate::filesystem::vfs::FSResult<String> {
        Ok(self.name)
    }
}

type RawFAT32Directory<'a> =
    fatfs::Dir<'a, Fat32RamDiskReader, fatfs::DefaultTimeProvider, fatfs::LossyOemCpConverter>;

#[derive(Debug)]
pub struct FAT32Directory {
    name: String,
    inner: RawFAT32Directory<'static>,
}

impl FAT32Directory {
    pub fn new(name: String, inner: RawFAT32Directory<'static>) -> Self {
        Self { name, inner }
    }
}

impl Directory for FAT32Directory {
    fn name(&self) -> crate::filesystem::vfs::FSResult<String> {
        Ok(self.name)
    }

    fn contents(
        &self,
    ) -> crate::filesystem::vfs::FSResult<
        alloc::vec::Vec<crate::filesystem::vfs_traits::DirectoryContentInfo>,
    > {
        let mut contents = Vec::new();

        for dir_entry in self.inner.iter() {
            contents.push(DirectoryContentInfo {
                name: dir_entry.unwrap().file_name(),
                content_type: if dir_entry.unwrap().is_file() {
                    DirectoryContentType::File
                } else {
                    DirectoryContentType::Directory
                },
            });
        }

        Ok(contents)
    }

    fn create(&self, info: DirectoryContentInfo) -> crate::filesystem::vfs::FSResult<()> {
        match info.content_type {
            DirectoryContentType::File => self.inner.create_file(&info.name).unwrap(),
            DirectoryContentType::Directory => self.inner.create_dir(&info.name).unwrap(),
            _ => unimplemented!(),
        }

        Ok(())
    }

    fn delete(&self, name: &str) -> crate::filesystem::vfs::FSResult<()> {
        self.inner.remove(name).map_err(|_| FSError::NotFound)
    }

    fn get(
        &self,
        name: &str,
    ) -> crate::filesystem::vfs::FSResult<crate::filesystem::vfs_traits::FileLike> {
        for dir_entry in self.inner.iter() {
            let dir_entry = dir_entry.unwrap();

            if dir_entry.file_name() == name {
                if dir_entry.is_file() {
                    return Ok(FileLike::File(Arc::new(Mutex::new(FAT32File::new(
                        dir_entry.file_name(),
                        dir_entry.to_file(),
                    )))));
                } else if dir_entry.is_dir() {
                    return Ok(FileLike::Directory(Arc::new(Mutex::new(
                        FAT32Directory::new(dir_entry.file_name(), dir_entry.to_dir()),
                    ))));
                }
            }
        }

        Err(FSError::NotFound)
    }
}

impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }
}

unsafe impl Sync for FAT32 {}
unsafe impl Sync for FAT32File {}
unsafe impl Send for FAT32File {}
unsafe impl Send for FAT32Directory {}
unsafe impl Sync for FAT32Directory {}
