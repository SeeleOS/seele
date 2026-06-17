# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Seele OS is an x86_64 operating system kernel written in `no_std` Rust targeting Linux binary compatibility — it runs unmodified Linux ELF binaries. The kernel boots via Limine, runs in QEMU (with KVM if `/dev/kvm` exists), and mounts an Arch Linux guest rootfs from `target/rootfs.img`. The rootfs installs a small base development package set through `xtask` and `pacstrap`.

## Commands

```sh
cargo xrun                  # build and boot in QEMU
cargo xrun -- --agent       # manual fallback headless boot; serial logs captured automatically
cargo xtest                 # kernel and integration tests in QEMU
cargo xbuild-rootfs         # build/refresh target/rootfs.img and rootfs
cargo xbuild-rootfs -- --override  # rebuild target/rootfs.img from scratch
cargo fmt --all             # format
cargo check --manifest-path kernel/Cargo.toml   # fast type-check kernel only
cargo clippy                # lint; treat all warnings as failures
```

When the `seele` MCP server is available, prefer it for VM workflows:

```text
run_tests                   # wrapper for cargo xtest
ensure_rootfs_mounted      # mount target/rootfs_mnt/ from target/rootfs.img when needed
start                 # build and launch the VM
status                # inspect runner/QEMU/QMP state
serial_tail           # read recent serial logs
screenshot            # capture display through QMP screendump
send_key/type_text    # drive guest input through QMP
stop or cleanup # stop the VM and remove QMP socket state
```

The `xtask/` crate implements every `cargo x*` subcommand. The local Rust toolchain is named `seele`; install it from `toolchain/install.rs` before building outside Nix. `nix develop` / `nix run` set up the full reproducible environment.

After any change: prefer the MCP workflow (`run_tests`, then `start`/status/serial/screenshot/stop). If MCP is unavailable, run `cargo xtest`, then `cargo xrun -- --agent`. The VM must boot cleanly before the work is done.

## Kernel Architecture

The kernel initializes in `kernel/src/lib.rs::init_kernel()` in this order:
boot → memory → SMP BSP → framebuffer/terminal/logging → early drivers → VFS → syscall MSRs → ACPI → thread/process manager → keyboard → interrupts → network → late drivers → APs released → scheduler loop.

### Module Map

| Path | Responsibility |
|---|---|
| `kernel/src/boot/` | Bootloader handoff, physical memory map, framebuffer info |
| `kernel/src/memory/` | Physical allocator, page tables, address spaces (`addrspace/`), heap |
| `kernel/src/smp/` | BSP/AP init, per-CPU GS base, GDT/TSS/IDT setup |
| `kernel/src/thread/` | Thread struct, scheduler, snapshots, signals |
| `kernel/src/process/` | Process manager, `execve`, ptrace, ELF loading |
| `kernel/src/systemcall/` | Syscall MSR setup, entry stub, dispatch table, all implementations |
| `kernel/src/filesystem/` | VFS core + ext4, tmpfs, procfs, cgroupfs, staticfs, devfs, sysfs, fusefs |
| `kernel/src/object/` | Kernel object model (fds, memfd, control objects) |
| `kernel/src/drivers/` | virtio-blk, e1000, PCI enumeration |
| `kernel/src/drm/` | DRM/KMS, PRIME buffer sharing |
| `kernel/src/evdev/` | evdev input device interface |
| `kernel/src/socket/` | Socket layer (TCP/UDP via smoltcp) |
| `kernel/src/net/` | Network stack initialization |
| `kernel/src/ipc/` | IPC primitives |
| `kernel/src/terminal/` | In-kernel framebuffer terminal (flanterm) |
| `xtask/src/` | All `cargo x*` subcommands: VM launch, rootfs build, test runner |
| `mcp/src/` | Seele MCP server: VM lifecycle, QMP screenshot/input, serial tail, and test wrappers |

### Syscall Path

```
syscall_entry (entry.rs, naked asm)
  → syscall_handler (handling.rs)
      → SYSCALL_TABLE[nr] (table.rs)
          → implementations/{filesystem,memory_sync,objects,signal,socket,...}.rs
```

Arguments are converted at the syscall boundary via `SyscallArg` traits in `arg_types.rs`. Typed flag types (bitflags) must be produced there, not inside handler bodies. The oracle for all ABI behavior is Linux x86_64 semantics.

### VFS

`VirtualFS` is a global locked `VirtualFileSystem` struct. Filesystems register mount points. The core traits live in `vfs_traits.rs`; path resolution in `resolve.rs`; per-fd operations in `vfs_operations.rs`. Each filesystem implementation lives under `filesystem/impls/` or its own subdirectory (`tmpfs/`, `procfs/`, `cgroupfs/`, etc.).

### Object Model

`kernel/src/object/` provides a typed handle table sitting above raw file descriptors. `control.rs` manages object lifecycle. `memfd.rs` implements memory-backed file objects.

### Memory

`kernel/src/memory/addrspace/` owns per-process virtual address spaces. `user.rs` exposes the user address space API; `mem_area.rs` tracks mapped regions; `paging.rs` manages page table entries. User pointer safety goes through `user_safe.rs`.

## Additional Guidelines

See `AGENTS.md` for operational rules covering: commit discipline, debugging workflow, VM interaction patterns, syscall logging, and testing requirements. Those rules apply here too.
