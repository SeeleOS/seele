use alloc::{collections::BTreeMap, vec::Vec};

use lazy_static::lazy_static;

use crate::{
    define_syscall,
    memory::{user_safe, utils::Mut},
    object::misc::get_object_current_process,
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
};

const IOCB_CMD_PWRITE: u16 = 1;
const IOCB_FLAG_RESFD: u32 = 1;
const AIO_CONTEXT_RING_MAGIC: u32 = 0xa10a10a1;
const AIO_CONTEXT_RING_HEAD_OFFSET: u64 = 0x08;
const AIO_CONTEXT_RING_TAIL_OFFSET: u64 = 0x0c;
const AIO_CONTEXT_RING_MAGIC_OFFSET: u64 = 0x10;

lazy_static! {
    static ref AIO_CONTEXTS: Mut<AioContexts> = Mut::new(AioContexts::default());
}

#[derive(Debug)]
struct AioContext {
    max_events: u32,
    completions: Vec<LinuxIoEvent>,
}

#[derive(Debug, Default)]
struct AioContexts {
    contexts: BTreeMap<u64, AioContext>,
}

impl AioContexts {
    fn create(&mut self, id: u64, max_events: u32) -> Result<(), SyscallError> {
        if max_events == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if id == 0 || self.contexts.contains_key(&id) {
            return Err(SyscallError::InvalidArguments);
        }

        self.contexts.insert(
            id,
            AioContext {
                max_events,
                completions: Vec::new(),
            },
        );
        Ok(())
    }

    fn destroy(&mut self, id: u64) -> Result<(), SyscallError> {
        let Some(context) = self.contexts.remove(&id) else {
            return Err(SyscallError::InvalidArguments);
        };
        let _ = context.max_events;
        Ok(())
    }

    fn context_mut(&mut self, id: u64) -> Result<&mut AioContext, SyscallError> {
        self.contexts
            .get_mut(&id)
            .ok_or(SyscallError::InvalidArguments)
    }
}

fn allocate_context_ring() -> Result<u64, SyscallError> {
    let ctx = {
        let process = get_current_process();
        let mut process = process.lock();
        process.addrspace.allocate_user(1).0.as_u64()
    };

    user_safe::write((ctx + AIO_CONTEXT_RING_HEAD_OFFSET) as *mut u32, &0_u32)?;
    user_safe::write((ctx + AIO_CONTEXT_RING_TAIL_OFFSET) as *mut u32, &0_u32)?;
    user_safe::write(
        (ctx + AIO_CONTEXT_RING_MAGIC_OFFSET) as *mut u32,
        &AIO_CONTEXT_RING_MAGIC,
    )?;

    Ok(ctx)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct LinuxIocb {
    aio_data: u64,
    aio_key: u32,
    aio_rw_flags: u32,
    aio_lio_opcode: u16,
    aio_reqprio: i16,
    aio_fildes: u32,
    aio_buf: u64,
    aio_nbytes: u64,
    aio_offset: i64,
    aio_reserved2: u64,
    aio_flags: u32,
    aio_resfd: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LinuxIoEvent {
    data: u64,
    obj: u64,
    res: i64,
    res2: i64,
}

fn submit_one(iocb_ptr: *const LinuxIocb) -> Result<LinuxIoEvent, SyscallError> {
    if iocb_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let iocb = user_safe::read(iocb_ptr)?;
    if iocb.aio_lio_opcode != IOCB_CMD_PWRITE || iocb.aio_nbytes > usize::MAX as u64 {
        return Err(SyscallError::InvalidArguments);
    }

    let object = get_object_current_process(u64::from(iocb.aio_fildes))?;
    let file = object.clone().as_file_like()?;
    let buffer = user_safe::read_buffer(iocb.aio_buf as *const u8, iocb.aio_nbytes as usize)?;
    let written = file.write_at(&buffer, iocb.aio_offset as u64)? as i64;

    if iocb.aio_flags & IOCB_FLAG_RESFD != 0 {
        let eventfd = get_object_current_process(u64::from(iocb.aio_resfd))?.as_eventfd()?;
        eventfd.notify_kernel_event();
    }

    Ok(LinuxIoEvent {
        data: iocb.aio_data,
        obj: iocb_ptr as u64,
        res: written,
        res2: 0,
    })
}

define_syscall!(IoSetup, |nr_events: u32, ctxp: *mut u64| {
    if ctxp.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if nr_events == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let ctx = allocate_context_ring()?;
    AIO_CONTEXTS.lock().create(ctx, nr_events)?;
    if let Err(err) = user_safe::write(ctxp, &ctx) {
        let _ = AIO_CONTEXTS.lock().destroy(ctx);
        return Err(err);
    }

    Ok(0)
});

define_syscall!(IoDestroy, |ctx: u64| {
    AIO_CONTEXTS.lock().destroy(ctx)?;
    Ok(0)
});

define_syscall!(
    IoSubmit,
    |ctx: u64, nr: i64, iocbpp: *const *const LinuxIocb| {
        if nr < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if nr == 0 {
            return Ok(0);
        }
        if iocbpp.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut submitted = 0usize;
        let mut completions = Vec::new();
        for index in 0..nr as usize {
            let iocb_ptr = user_safe::read(unsafe { iocbpp.add(index) })?;
            match submit_one(iocb_ptr) {
                Ok(event) => {
                    completions.push(event);
                    submitted += 1;
                }
                Err(err) if submitted == 0 => return Err(err),
                Err(_) => break,
            }
        }

        let mut contexts = AIO_CONTEXTS.lock();
        let context = contexts.context_mut(ctx)?;
        if context.completions.len() + completions.len() > context.max_events as usize {
            return Err(SyscallError::TryAgain);
        }
        context.completions.extend(completions);
        Ok(submitted)
    }
);

define_syscall!(
    IoGetevents,
    |ctx: u64, min_nr: i64, nr: i64, events: *mut LinuxIoEvent, _timeout: *const u8| {
        if min_nr < 0 || nr < 0 || min_nr > nr {
            return Err(SyscallError::InvalidArguments);
        }
        if nr == 0 {
            return Ok(0);
        }
        if events.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let ready = {
            let mut contexts = AIO_CONTEXTS.lock();
            let context = contexts.context_mut(ctx)?;
            let count = (nr as usize).min(context.completions.len());
            context.completions.drain(..count).collect::<Vec<_>>()
        };

        if ready.len() < min_nr as usize {
            return Err(SyscallError::TryAgain);
        }

        for (index, event) in ready.iter().enumerate() {
            user_safe::write(unsafe { events.add(index) }, event)?;
        }
        Ok(ready.len())
    }
);

define_syscall!(
    IoCancel,
    |_ctx: u64, _iocb: *const LinuxIocb, _result: *mut LinuxIoEvent| {
        Err(SyscallError::InvalidArguments)
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemcall::{
        test_helpers::{
            SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
        },
        utils::SyscallError,
    };

    crate::test!(
        aio_context_syscalls,
        "aio context syscalls follow linux lifecycle rules",
        aio_context_syscalls_follow_linux_lifecycle_rules
    );

    fn aio_context_syscalls_follow_linux_lifecycle_rules() {
        let page = allocate_user_test_page();

        expect_errno(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<IoSetup>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0, page, 0, 0, 0, 0]).call::<IoSetup>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([16, page, 0, 0, 0, 0]).call::<IoSetup>(),
            0,
        );
        let ctx = read_user_value::<u64>(page);
        assert_ne!(ctx, 0);
        assert_eq!(
            read_user_value::<u32>(ctx + AIO_CONTEXT_RING_MAGIC_OFFSET),
            AIO_CONTEXT_RING_MAGIC
        );
        assert_eq!(
            read_user_value::<u32>(ctx + AIO_CONTEXT_RING_HEAD_OFFSET),
            0
        );
        assert_eq!(
            read_user_value::<u32>(ctx + AIO_CONTEXT_RING_TAIL_OFFSET),
            0
        );

        expect_ok(
            SyscallArgs::new([ctx, 0, 0, 0, 0, 0]).call::<IoDestroy>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([ctx, 0, 0, 0, 0, 0]).call::<IoDestroy>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<IoDestroy>(),
            SyscallError::InvalidArguments,
        );
    }
}
