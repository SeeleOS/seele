use crate::memory::utils::Mut;
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::mem::size_of;

use crate::{
    impl_cast_function_non_trait,
    object::{
        FileFlags, Object,
        misc::{ObjectResult, get_object_current_process},
        open_state::OpenState,
    },
    systemcall::utils::{SyscallError, SyscallResult},
};

const BPF_MAP_TYPE_ARRAY: u32 = 2;
const BPF_MAP_TYPE_RINGBUF: u32 = 27;
const BPF_PSEUDO_MAP_FD: u8 = 1;
const BPF_FUNC_MAP_LOOKUP_ELEM: i32 = 1;

const BPF_LD: u8 = 0x00;
const BPF_ST: u8 = 0x02;
const BPF_STX: u8 = 0x03;
const BPF_ALU: u8 = 0x04;
const BPF_JMP: u8 = 0x05;
const BPF_ALU64: u8 = 0x07;
const BPF_DW: u8 = 0x18;
const BPF_MEM: u8 = 0x60;
const BPF_IMM: u8 = 0x00;
const BPF_X: u8 = 0x08;
const BPF_ADD: u8 = 0x00;
const BPF_SUB: u8 = 0x10;
const BPF_MUL: u8 = 0x20;
const BPF_DIV: u8 = 0x30;
const BPF_LSH: u8 = 0x60;
const BPF_RSH: u8 = 0x70;
const BPF_MOD: u8 = 0x90;
const BPF_MOV: u8 = 0xb0;
const BPF_JEQ: u8 = 0x10;
const BPF_JNE: u8 = 0x50;
const BPF_CALL: u8 = 0x80;
const BPF_EXIT: u8 = 0x90;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BpfInsn {
    code: u8,
    regs: u8,
    off: i16,
    imm: i32,
}

#[derive(Debug)]
enum BpfObjectKind {
    Program(BpfProgramState),
    Map(BpfMapState),
}

#[derive(Debug)]
struct BpfProgramState {
    prog_type: u32,
    insns: Vec<BpfInsn>,
}

#[derive(Debug)]
struct BpfMapState {
    map_type: u32,
    key_size: usize,
    value_size: usize,
    max_entries: usize,
    entries: Mut<BTreeMap<Vec<u8>, Vec<u8>>>,
}

#[derive(Debug)]
pub struct BpfObject {
    kind: BpfObjectKind,
    open_state: OpenState,
}

impl BpfObject {
    pub fn new_program(prog_type: u32, insns: Vec<BpfInsn>) -> Arc<Self> {
        Arc::new(Self {
            kind: BpfObjectKind::Program(BpfProgramState { prog_type, insns }),
            open_state: OpenState::default(),
        })
    }

    pub fn new_map(map_type: u32, key_size: u32, value_size: u32, max_entries: u32) -> Arc<Self> {
        Arc::new(Self {
            kind: BpfObjectKind::Map(BpfMapState {
                map_type,
                key_size: key_size as usize,
                value_size: value_size as usize,
                max_entries: max_entries as usize,
                entries: Mut::new(BTreeMap::new()),
            }),
            open_state: OpenState::default(),
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

    pub fn verify_program(insns: &[BpfInsn]) -> Result<(), &'static str> {
        let mut regs = [VerifierReg::Scalar; 11];
        regs[10] = VerifierReg::StackPtr;

        let mut pc = 0;
        while pc < insns.len() {
            let insn = insns[pc];
            let dst = insn.dst_reg() as usize;
            let src = insn.src_reg() as usize;
            match (insn.class(), insn.op(), insn.mode()) {
                (BPF_LD, _, BPF_IMM) if (insn.code & BPF_DW) == BPF_DW => {
                    if pc + 1 >= insns.len() {
                        return Err("truncated ldimm64");
                    }
                    regs[dst] = if insn.src_reg() == BPF_PSEUDO_MAP_FD {
                        VerifierReg::Map
                    } else {
                        VerifierReg::Scalar
                    };
                    pc += 2;
                    continue;
                }
                (BPF_ALU | BPF_ALU64, BPF_MOV, _) if insn.source() == BPF_X => {
                    regs[dst] = regs[src]
                }
                (BPF_ALU | BPF_ALU64, BPF_MOV, _) => regs[dst] = VerifierReg::Scalar,
                (BPF_ALU64, BPF_ADD | BPF_SUB, _) if matches!(regs[dst], VerifierReg::StackPtr) => {
                }
                (BPF_ALU64, BPF_ADD | BPF_SUB, _)
                    if matches!(regs[dst], VerifierReg::MapValuePtr) =>
                {
                    return Err("map value pointer arithmetic is not allowed");
                }
                (BPF_ALU | BPF_ALU64, _, _) => regs[dst] = VerifierReg::Scalar,
                (BPF_ST, _, BPF_MEM) => {
                    if !matches!(regs[dst], VerifierReg::StackPtr | VerifierReg::MapValuePtr) {
                        return Err("store destination is not writable");
                    }
                }
                (BPF_STX, _, BPF_MEM) => {
                    if !matches!(regs[dst], VerifierReg::StackPtr | VerifierReg::MapValuePtr) {
                        return Err("store destination is not writable");
                    }
                }
                (BPF_JMP, BPF_CALL, _) => {
                    if insn.imm != BPF_FUNC_MAP_LOOKUP_ELEM {
                        return Err("unsupported or unsafe helper");
                    }
                    if !matches!(regs[1], VerifierReg::Map)
                        || !matches!(regs[2], VerifierReg::StackPtr)
                    {
                        return Err("invalid map_lookup_elem arguments");
                    }
                    regs[0] = VerifierReg::MapValuePtr;
                }
                (BPF_JMP, BPF_EXIT | BPF_JEQ | BPF_JNE, _) => {}
                _ => {}
            }
            pc += 1;
        }
        Ok(())
    }

    pub fn run_socket_filter(&self, _packet: &[u8]) -> SyscallResult<u64> {
        let program = match &self.kind {
            BpfObjectKind::Program(program) => program,
            BpfObjectKind::Map(_) => return Err(SyscallError::BadFileDescriptor),
        };
        BpfInterpreter::new(&program.insns).run()
    }

    fn map_state(&self) -> SyscallResult<&BpfMapState> {
        match &self.kind {
            BpfObjectKind::Program(_) => Err(SyscallError::BadFileDescriptor),
            BpfObjectKind::Map(map) => Ok(map),
        }
    }

    fn write_map_element_at(&self, key: &[u8], offset: usize, bytes: &[u8]) -> SyscallResult<()> {
        let map = self.map_state()?;
        map.validate_key(key)?;
        if offset
            .checked_add(bytes.len())
            .is_none_or(|end| end > map.value_size)
        {
            return Err(SyscallError::InvalidArguments);
        }

        if map.map_type == BPF_MAP_TYPE_ARRAY {
            let index = map.array_index(key)?;
            if index >= map.max_entries {
                return Err(SyscallError::FileNotFound);
            }
        }

        let mut entries = map.entries.lock();
        let value = entries
            .entry(key.to_vec())
            .or_insert_with(|| alloc::vec![0; map.value_size]);
        value[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
}

impl BpfMapState {
    fn validate_key(&self, key: &[u8]) -> SyscallResult<()> {
        if self.map_type == BPF_MAP_TYPE_RINGBUF {
            return Err(SyscallError::OperationNotSupported);
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerifierReg {
    Scalar,
    StackPtr,
    Map,
    MapValuePtr,
}

#[derive(Clone)]
enum BpfValue {
    Scalar(u64),
    StackPtr(i64),
    Map(Arc<BpfObject>),
    MapValuePtr {
        map: Arc<BpfObject>,
        key: Vec<u8>,
        offset: i64,
    },
}

impl BpfValue {
    fn scalar(&self) -> u64 {
        match self {
            Self::Scalar(value) => *value,
            Self::StackPtr(_) | Self::Map(_) | Self::MapValuePtr { .. } => 1,
        }
    }
}

struct BpfInterpreter<'a> {
    insns: &'a [BpfInsn],
    regs: Vec<BpfValue>,
    stack: [u8; 512],
}

impl<'a> BpfInterpreter<'a> {
    fn new(insns: &'a [BpfInsn]) -> Self {
        let mut regs = alloc::vec![BpfValue::Scalar(0); 11];
        regs[10] = BpfValue::StackPtr(0);
        Self {
            insns,
            regs,
            stack: [0; 512],
        }
    }

    fn run(&mut self) -> SyscallResult<u64> {
        let mut pc = 0usize;
        let mut steps = 0usize;
        while pc < self.insns.len() && steps < 4096 {
            steps += 1;
            let insn = self.insns[pc];
            let dst = insn.dst_reg() as usize;
            let src = insn.src_reg() as usize;
            match (insn.class(), insn.op(), insn.mode()) {
                (BPF_LD, _, BPF_IMM) if (insn.code & BPF_DW) == BPF_DW => {
                    if pc + 1 >= self.insns.len() {
                        return Err(SyscallError::InvalidArguments);
                    }
                    let imm =
                        (insn.imm as u32 as u64) | ((self.insns[pc + 1].imm as u32 as u64) << 32);
                    self.regs[dst] = if insn.src_reg() == BPF_PSEUDO_MAP_FD {
                        let map = get_object_current_process(imm)?.as_bpf()?;
                        BpfValue::Map(map)
                    } else {
                        BpfValue::Scalar(imm)
                    };
                    pc += 2;
                    continue;
                }
                (BPF_ALU | BPF_ALU64, BPF_MOV, _) if insn.source() == BPF_X => {
                    self.regs[dst] = self.regs[src].clone()
                }
                (BPF_ALU | BPF_ALU64, BPF_MOV, _) => {
                    self.regs[dst] = BpfValue::Scalar(insn.imm as i64 as u64)
                }
                (BPF_ALU | BPF_ALU64, op, _) => {
                    self.apply_alu(dst, src, op, insn.source() == BPF_X, insn.imm);
                }
                (BPF_ST, _, BPF_MEM) => {
                    self.write_value(dst, insn.off, insn.imm as i64 as u64, insn.size())?;
                }
                (BPF_STX, _, BPF_MEM) => {
                    self.write_value(dst, insn.off, self.regs[src].scalar(), insn.size())?;
                }
                (BPF_JMP, BPF_CALL, _) => {
                    if insn.imm == BPF_FUNC_MAP_LOOKUP_ELEM {
                        self.map_lookup()?;
                    } else {
                        return Err(SyscallError::OperationNotSupported);
                    }
                }
                (BPF_JMP, BPF_JEQ | BPF_JNE, _) => {
                    let left = self.regs[dst].scalar();
                    let right = if insn.source() == BPF_X {
                        self.regs[src].scalar()
                    } else {
                        insn.imm as i64 as u64
                    };
                    let equal = left == right;
                    if (insn.op() == BPF_JEQ && equal) || (insn.op() == BPF_JNE && !equal) {
                        pc = pc.wrapping_add(1).wrapping_add(insn.off as isize as usize);
                        continue;
                    }
                }
                (BPF_JMP, BPF_EXIT, _) => return Ok(self.regs[0].scalar()),
                _ => {}
            }
            pc += 1;
        }
        Ok(self.regs[0].scalar())
    }

    fn apply_alu(&mut self, dst: usize, src: usize, op: u8, source_is_reg: bool, imm: i32) {
        let rhs = if source_is_reg {
            self.regs[src].scalar()
        } else {
            imm as i64 as u64
        };
        match (&mut self.regs[dst], op) {
            (BpfValue::StackPtr(offset), BPF_ADD) => *offset += rhs as i64,
            (BpfValue::StackPtr(offset), BPF_SUB) => *offset -= rhs as i64,
            (BpfValue::Scalar(value), BPF_ADD) => *value = value.wrapping_add(rhs),
            (BpfValue::Scalar(value), BPF_SUB) => *value = value.wrapping_sub(rhs),
            (BpfValue::Scalar(value), BPF_MUL) => *value = value.wrapping_mul(rhs),
            (BpfValue::Scalar(value), BPF_LSH) => *value = value.wrapping_shl(rhs as u32),
            (BpfValue::Scalar(value), BPF_RSH) => *value = value.wrapping_shr(rhs as u32),
            (BpfValue::Scalar(value), BPF_DIV) => {
                *value = (*value as u32).checked_div(rhs as u32).unwrap_or(0) as u64;
            }
            (BpfValue::Scalar(value), BPF_MOD) => {
                if (rhs as u32) != 0 {
                    *value = ((*value as u32) % (rhs as u32)) as u64;
                } else {
                    *value = *value as u32 as u64;
                }
            }
            _ => self.regs[dst] = BpfValue::Scalar(0),
        }
    }

    fn map_lookup(&mut self) -> SyscallResult<()> {
        let map = match &self.regs[1] {
            BpfValue::Map(map) => map.clone(),
            _ => return Err(SyscallError::InvalidArguments),
        };
        let key_size = map.map_key_size()?;
        let key = self.read_stack_ptr(2, key_size)?;
        let _ = map.lookup_map_element(&key)?;
        self.regs[0] = BpfValue::MapValuePtr {
            map,
            key,
            offset: 0,
        };
        Ok(())
    }

    fn read_stack_ptr(&self, reg: usize, len: usize) -> SyscallResult<Vec<u8>> {
        let offset = match self.regs[reg] {
            BpfValue::StackPtr(offset) => offset,
            _ => return Err(SyscallError::InvalidArguments),
        };
        let start = stack_index(offset)?;
        let end = start
            .checked_add(len)
            .ok_or(SyscallError::InvalidArguments)?;
        if end > self.stack.len() {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(self.stack[start..end].to_vec())
    }

    fn write_value(
        &mut self,
        dst: usize,
        insn_offset: i16,
        value: u64,
        size: usize,
    ) -> SyscallResult<()> {
        let bytes = value.to_ne_bytes();
        match &self.regs[dst] {
            BpfValue::StackPtr(offset) => {
                let start = stack_index(*offset + insn_offset as i64)?;
                let end = start
                    .checked_add(size)
                    .ok_or(SyscallError::InvalidArguments)?;
                if end > self.stack.len() || size > bytes.len() {
                    return Err(SyscallError::InvalidArguments);
                }
                self.stack[start..end].copy_from_slice(&bytes[..size]);
                Ok(())
            }
            BpfValue::MapValuePtr { map, key, offset } => {
                let offset = (*offset + insn_offset as i64)
                    .try_into()
                    .map_err(|_| SyscallError::InvalidArguments)?;
                map.write_map_element_at(key, offset, &bytes[..size])
            }
            _ => Err(SyscallError::InvalidArguments),
        }
    }
}

fn stack_index(offset: i64) -> SyscallResult<usize> {
    if !(-512..=0).contains(&offset) {
        return Err(SyscallError::InvalidArguments);
    }
    usize::try_from(512 + offset).map_err(|_| SyscallError::InvalidArguments)
}

impl BpfInsn {
    fn class(self) -> u8 {
        self.code & 0x07
    }

    fn op(self) -> u8 {
        self.code & 0xf0
    }

    fn mode(self) -> u8 {
        self.code & 0xe0
    }

    fn source(self) -> u8 {
        self.code & 0x08
    }

    fn size(self) -> usize {
        match self.code & 0x18 {
            0x00 => 4,
            0x08 => 2,
            0x10 => 1,
            0x18 => 8,
            _ => 0,
        }
    }

    fn dst_reg(self) -> u8 {
        self.regs & 0x0f
    }

    fn src_reg(self) -> u8 {
        self.regs >> 4
    }
}

impl From<[u8; 8]> for BpfInsn {
    fn from(value: [u8; 8]) -> Self {
        Self {
            code: value[0],
            regs: value[1],
            off: i16::from_ne_bytes([value[2], value[3]]),
            imm: i32::from_ne_bytes([value[4], value[5], value[6], value[7]]),
        }
    }
}

impl Object for BpfObject {
    fn debug_name(&self) -> &'static str {
        "bpf"
    }

    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
    }

    impl_cast_function_non_trait!("bpf", BpfObject);
}
