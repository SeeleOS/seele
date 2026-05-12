# `gcc --version` / `rustc --version` crash root-cause report

## Scope

This report is for diagnosis only. It does not propose or apply a fix.

## Stable reproduction

1. Ensure `sysroot` is not mounted while the VM is running.
2. Ensure no leftover runner or QEMU process remains.
3. Start `agent-tools/run-agent-vm.sh`.
4. Wait for `Seele login:`, log in as `root`.
5. Run `gcc --version` in a fresh VM.
6. Run `rustc --version` in a fresh VM.

Expected result:

- `gcc --version` ends with `Segmentation fault`.
- `rustc --version` ends with `Segmentation fault`.
- `clang --version` works.
- `/lib/ld-linux-x86-64.so.2 --help` works.
- `ldd /usr/bin/gcc` and `ldd /usr/bin/rustc` work.

The control samples matter: this is not a broad `execve` failure and not a broad dynamic-linker path failure.

## Crash summaries

`gcc --version`

- Last traced syscalls before the first fault were successful `brk`, `mmap`, `arch_prctl(ARCH_SET_FS)`, three `mprotect`, then `munmap`.
- First real fault was user-mode instruction fetch from `0x0`.
- Observed fault summary:
  `rip=0x0 cr2=0x0 err=USER_MODE|INSTRUCTION_FETCH last_syscall=munmap rax=0x5a64d0`
- `0x5a64d0` is exactly `gcc`'s `.fini_array`.

`rustc --version`

- Last traced syscalls before the first fault were successful `mmap`, `arch_prctl(ARCH_SET_FS)`, many `mprotect`, one `munmap`, then three successful `brk` calls.
- First real fault was user-mode write to `0x38` while `rip` still pointed at the PIE entrypoint.
- Observed fault summary:
  `rip=0xc00017b0 cr2=0x38 err=CAUSED_BY_WRITE|USER_MODE last_syscall=brk rax=0x38`
- `0xc00017b0` is exactly `rustc`'s loaded `_start`.

These are different immediate crash shapes, but they converge on the same kernel bug.

## Confirmed direct root cause

The direct root cause is in the shared lazy file-page cache for non-writable file-backed mappings in [kernel/src/memory/addrspace/apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:22).

Relevant behavior:

- The cache key is only `(device_id, inode, offset)` in [apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:22).
- `get_or_load_shared_file_frame()` caches a frame after reading only `read_len` bytes and zero-filling the rest of the 4 KiB page in [apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:86).
- Any non-writable file-backed area uses that shared cache in [apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:132).
- ELF PT_LOAD mapping aligns each segment down to page boundaries and records `file_bytes = p_filesz + page_delta` in [kernel/src/elfloader/segment.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/elfloader/segment.rs:23).

That combination is wrong when two PT_LOAD segments map the same 4 KiB file page but require different valid byte counts from that page.

What goes wrong:

1. The first fault on file page `X` can cache only the prefix required by one PT_LOAD and zero the rest of the frame.
2. A later fault on another PT_LOAD that also maps file page `X` reuses the same cached frame because the key ignores `read_len` and ignores which PT_LOAD view is asking.
3. Bytes that should have come from the file are already permanently replaced with zeros.

This is enough to corrupt executable bytes, GOT entries, init/fini arrays, and RELRO data without any syscall returning an error.

## `rustc` direct cause

`rustc` is a tiny PIE wrapper with these PT_LOAD ranges:

- `LOAD off 0x000000 vaddr 0x000000 filesz 0x770 memsz 0x770 R`
- `LOAD off 0x000770 vaddr 0x0001770 filesz 0x1a5 memsz 0x1a5 R E`

Both PT_LOADs depend on file page offset `0x0`.

What the kernel computes:

- For the first PT_LOAD, the cached file-page prefix can be only `0x770` bytes.
- For the executable PT_LOAD, the same file page needs `0x915` bytes because `page_delta = 0x770` and `file_bytes = 0x770 + 0x1a5 = 0x915`.

The entrypoint is `0x17b0`. That corresponds to file offset `0x7b0`, which lies inside the range `0x770..0x914`.

If the first PT_LOAD faults first, the cache stores file page `0x0` with bytes `0x770..0xfff` zeroed. The executable PT_LOAD then reuses that truncated page. `_start` begins executing zeros instead of real instructions.

That exactly explains the observed first fault:

- `rip` stays at `_start` because the very first bytes fetched from `_start` are wrong.
- Zero bytes decode as normal x86 instructions such as `add byte ptr [rax], al`.
- With `rax = 0x38`, the first visible fault becomes a write fault on `0x38`.

So `rustc` is not primarily failing in TLS, signals, or `brk`; it is executing a corrupted first code page.

## `gcc` direct cause

`gcc` has these relevant PT_LOADs:

- `LOAD off 0x0dc000 vaddr 0x4dc000 filesz 0x0c9438 memsz 0x0c9438 R`
- `LOAD off 0x1a54b8 vaddr 0x5a64b8 filesz 0x0025f0 memsz 0x006a50 RW`
- `GNU_RELRO off 0x1a54b8 vaddr 0x5a64b8 filesz 0x001b48`

The readonly PT_LOAD ends in file page `0x1a5000`.

What the kernel computes:

- The last page of the readonly PT_LOAD needs only `0x438` bytes from file page `0x1a5000`.
- The first page of the RW PT_LOAD needs the full first `0x1000` bytes from the same file page `0x1a5000`.

The first page of the RW PT_LOAD contains:

- `.init_array` at file offset `0x1a54b8`, page offset `0x4b8`
- `.fini_array` at file offset `0x1a54d0`, page offset `0x4d0`
- `.data.rel.ro` at file offset `0x1a54e0`, page offset `0x4e0`

All three are beyond `0x438`. If the readonly PT_LOAD cached file page `0x1a5000` first, those bytes become zero in the cached frame.

Why `mprotect` shows up right before the crash:

- The RW PT_LOAD starts writable, so it would not use the shared readonly cache before RELRO.
- The loader then applies `mprotect(PROT_READ)` to the RELRO range.
- If the first touch of that page happens after `mprotect`, the page fault path now treats it as non-writable and reuses the already-truncated shared frame.

That matches the observed `gcc` sequence:

- `arch_prctl` succeeds.
- RELRO `mprotect` calls succeed.
- `munmap` succeeds.
- Later startup logic uses now-corrupted array or RELRO contents.
- The fault log reports `rax=0x5a64d0`, which is exactly `.fini_array`.
- The eventual visible crash is an indirect transfer to `0x0`, producing `rip=0x0` and instruction fetch fault at `cr2=0x0`.

So `gcc` is not primarily failing because `munmap` removed the wrong region. The bad data was already present because the RELRO page reused a truncated cached file page.

## Why `clang` survives

`clang` does not trigger the same bug on its startup path because its adjacent PT_LOAD segments do not reuse the same file page across a segment boundary:

- `RX` begins at file offset `0x00b000`, page aligned.
- The following `R` segment ends before file page `0x2a000`.
- The `RW` segment begins at file offset `0x02a038`, whose aligned page `0x02a000` is not shared with the previous PT_LOAD.

The buggy cache logic still exists, but `clang` does not step on it during its initial startup mapping.

## Contributing conditions

- Lazy file-backed faults are enabled, so the first PT_LOAD that touches a shared file page seeds the cache for later PT_LOADs.
- The shared cache is used for all non-writable mappings, not only genuinely shared mappings.
- ELF PT_LOAD boundaries are page-aligned by the loader, so adjacent segments can legitimately refer to the same underlying file page with different valid byte ranges.
- `GNU_RELRO` turns the front of an originally writable segment into a non-writable segment before first touch, which exposes the bug on data pages like `gcc`'s `.fini_array`.

## Major candidates that were ruled out

| Cluster | Status | Conclusion |
| --- | --- | --- |
| Context switch / FP / SIMD / XSAVE | ruled out | First real failures are plain user page faults caused by corrupted mapped bytes, not FP exceptions or state restore failures. |
| `execve` / ELF / initial stack / auxv | confirmed in ELF mapping subpath | The problem is in ELF PT_LOAD page sharing, not in argv/env/auxv layout. Control samples show the general exec path works. |
| Virtual memory: `mmap` / `mprotect` / `munmap` / lazy mapping / page fault | confirmed | This is the failing subsystem. The key bug is lazy file-page caching across distinct PT_LOAD views. |
| TLS / `arch_prctl` / FS base | ruled out | `ARCH_SET_FS` succeeds and the recorded `fs_base` stays coherent. The crash is already explained by corrupted code/data pages. |
| Signal / `rt_sigreturn` / `ucontext` | ruled out | The first failure happens before signal delivery. The shell cleanly receives `SIGSEGV` afterward. |
| Syscall ABI: registers / errno / return values | ruled out as direct cause | Traced `mmap`, `mprotect`, `munmap`, `brk`, and `arch_prctl` return success with plausible addresses. No syscall ABI mismatch is needed to explain the crash. |
| Filesystem and dynamic-linker path | ruled out | `ld.so --help`, `ldd`, and other dynamic-linker control cases work. The issue is not missing files or broken lookup paths. |
| Scheduling / locks / deadlock / re-entry | ruled out | One temporary hang came from debug logging lock scope, not from the original crash. The native `gcc` and `rustc` failures still reduce to corrupted file-backed pages. |

## Likely fix areas

If someone implements a fix, the first places to inspect are:

- [kernel/src/memory/addrspace/apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:22)
- [kernel/src/elfloader/segment.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/elfloader/segment.rs:23)
- [kernel/src/memory/addrspace/mod.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/mod.rs:196)

The fix almost certainly belongs in shared file-page cache semantics, not in signals, scheduler code, or userspace packaging.

## Shortest confirmation path for the implementer

The shortest deterministic confirmation path is:

1. Reproduce `gcc --version` and `rustc --version` in fresh VMs.
2. Break in `AddrSpace::get_or_load_shared_file_frame()` in [apply.rs](/home/elysia/coding-project/seele-os-linux/kernel/src/memory/addrspace/apply.rs:86).
3. For `rustc`, observe the same `(inode, offset=0x0)` reused with two different required byte counts: first `0x770`, later `0x915`.
4. For `gcc`, observe file page `offset=0x1a5000` reused first with `0x438`, later with `0x1000` after RELRO has made the page non-writable.
5. Continue to the first user page fault and confirm it matches the crash summaries above.

That is sufficient to re-prove the bug without adding broad syscall logging.
