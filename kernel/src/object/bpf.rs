use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::mem::size_of;
use spin::Mutex;

use crate::{
    impl_cast_function_non_trait,
    object::Object,
    systemcall::utils::{SyscallError, SyscallResult},
};

const BPF_MAP_TYPE_ARRAY: u32 = 2;

#[derive(Debug)]
enum BpfObjectKind {
    Program(BpfProgramState),
    Map(BpfMapState),
}

#[derive(Debug)]
struct BpfProgramState {
    prog_type: u32,
}

#[derive(Debug)]
struct BpfMapState {
    map_type: u32,
    key_size: usize,
    value_size: usize,
    max_entries: usize,
    entries: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
}

#[derive(Debug)]
pub struct BpfObject {
    kind: BpfObjectKind,
}

impl BpfObject {
    pub fn new_program(prog_type: u32) -> Arc<Self> {
        Arc::new(Self {
            kind: BpfObjectKind::Program(BpfProgramState { prog_type }),
        })
    }

    pub fn new_map(map_type: u32, key_size: u32, value_size: u32, max_entries: u32) -> Arc<Self> {
        Arc::new(Self {
            kind: BpfObjectKind::Map(BpfMapState {
                map_type,
                key_size: key_size as usize,
                value_size: value_size as usize,
                max_entries: max_entries as usize,
                entries: Mutex::new(BTreeMap::new()),
            }),
        })
    }

    pub fn prog_type(&self) -> SyscallResult<u32> {
        match &self.kind {
            BpfObjectKind::Program(program) => Ok(program.prog_type),
            BpfObjectKind::Map(_) => Err(SyscallError::BadFileDescriptor),
        }
    }

    pub fn map_key_size(&self) -> SyscallResult<usize> {
        Ok(self.map_state()?.key_size)
    }

    pub fn map_value_size(&self) -> SyscallResult<usize> {
        Ok(self.map_state()?.value_size)
    }

    pub fn update_map_element(&self, key: &[u8], value: &[u8]) -> SyscallResult<()> {
        let map = self.map_state()?;
        map.validate_key(key)?;
        if value.len() != map.value_size {
            return Err(SyscallError::InvalidArguments);
        }

        if map.map_type == BPF_MAP_TYPE_ARRAY {
            let index = map.array_index(key)?;
            if index >= map.max_entries {
                return Err(SyscallError::InvalidArguments);
            }
        } else {
            let mut entries = map.entries.lock();
            if entries.len() >= map.max_entries && !entries.contains_key(key) {
                return Err(SyscallError::NoSpaceLeft);
            }

            entries.insert(key.to_vec(), value.to_vec());
            return Ok(());
        }

        map.entries.lock().insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    pub fn lookup_map_element(&self, key: &[u8]) -> SyscallResult<Vec<u8>> {
        let map = self.map_state()?;
        map.validate_key(key)?;

        if map.map_type == BPF_MAP_TYPE_ARRAY {
            let index = map.array_index(key)?;
            if index >= map.max_entries {
                return Err(SyscallError::FileNotFound);
            }
            return Ok(map
                .entries
                .lock()
                .get(key)
                .cloned()
                .unwrap_or_else(|| alloc::vec![0; map.value_size]));
        }

        map.entries
            .lock()
            .get(key)
            .cloned()
            .ok_or(SyscallError::FileNotFound)
    }

    fn map_state(&self) -> SyscallResult<&BpfMapState> {
        match &self.kind {
            BpfObjectKind::Program(_) => Err(SyscallError::BadFileDescriptor),
            BpfObjectKind::Map(map) => Ok(map),
        }
    }
}

impl BpfMapState {
    fn validate_key(&self, key: &[u8]) -> SyscallResult<()> {
        if key.len() != self.key_size {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(())
    }

    fn array_index(&self, key: &[u8]) -> SyscallResult<usize> {
        if key.len() != size_of::<u32>() {
            return Err(SyscallError::InvalidArguments);
        }

        let index = u32::from_ne_bytes(key.try_into().unwrap()) as usize;
        Ok(index)
    }
}

impl Object for BpfObject {
    fn debug_name(&self) -> &'static str {
        "bpf"
    }

    impl_cast_function_non_trait!("bpf", BpfObject);
}
