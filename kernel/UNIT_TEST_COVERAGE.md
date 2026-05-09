# Kernel Unit Test Coverage

This ledger tracks meaningful kernel unit-test coverage. It is not a line
coverage target: hardware programming, boot sequencing, MMU side effects,
interrupt execution, scheduler switching, and full userspace flows stay under
the QEMU unit and integration harnesses.

Update this file when adding or deliberately deferring kernel tests.

## Status Key

- `unit-tested`: Pure logic, ABI conversion, state transitions, or object-local
  behavior has focused tests under `kernel/src/**/test.rs`.
- `integration-only`: Behavior depends on real boot state, devices, privileged
  CPU state, global process/thread state, or userspace execution.
- `not worth testing`: Trivial glue, declarations, module wiring, or code whose
  only behavior is delegated elsewhere.

## Coverage Ledger

| Area | Status | Notes |
| --- | --- | --- |
| `filesystem/path.rs`, `absolute_path.rs`, `sparse_file.rs`, `info.rs`, `vfs_traits.rs` | `unit-tested` | Path normalization, root-relative resolution, sparse hole reads, Linux stat mode conversion, and mount option rendering are covered in `filesystem/test.rs`. |
| `filesystem/tmpfs/state.rs` | `unit-tested` | Child creation, duplicate detection, and non-empty directory removal are covered. |
| `filesystem/staticfs` | `unit-tested` | Static lookup/tree shape, file/symlink metadata, readonly `rename`/`link`, and default mount metadata are covered in `filesystem/test.rs`. |
| `filesystem/sysfs` | `unit-tested` | Static tree shape, mount metadata, readonly behavior, and `uevent` payload parsing/building are covered in `filesystem/test.rs` and `sysfs/mod.rs` tests. Live `tty0/active` content and netlink broadcast side effects still rely on runtime integration. |
| `filesystem/devfs` | `unit-tested` | Static root seed paths, overlay path normalization, static-path gating, `/pts` metadata, symlink targets, and readonly protection of static nodes are covered in `filesystem/test.rs` and `devfs/mod.rs` tests. Dynamic PTY population and backing device IO remain runtime-driven. |
| `filesystem/procfs` | `unit-tested` | Static directory/file trees under `/proc`, `/proc/sys`, `/proc/pressure`, `/proc/net`, pure formatting/parsing helpers, UUID/layout helpers, namespace name tables, and mount metadata are covered in `filesystem/test.rs` plus internal procfs tests. Per-process views, mount snapshots, uptime/stat counters, and live fd/process state remain integration-driven. |
| `filesystem/cgroupfs` | `integration-only` | Visible behavior still depends on live process/cgroup runtime state. |
| `filesystem/vfs.rs`, `vfs_operations.rs`, `resolve.rs` | `integration-only` | VFS side effects depend on global mounted filesystems and process context. Add pure helper tests when helpers are extracted. |
| `filesystem/impls/ext4` | `integration-only` | Real filesystem behavior depends on disk data. Only local pure helpers such as mode/cache transforms should be unit-tested. |
| `memory/paging.rs`, `protection.rs` | `unit-tested` | Page count normalization, 4 KiB alignment, and protection flag closure are covered. |
| `memory/addrspace`, `user_safe.rs`, `page_table_wrapper.rs` | `integration-only` | Depends on page tables, active address spaces, and user memory access effects. Pure range helpers should be added when available. |
| `elfloader/util.rs`, `segment.rs` | `unit-tested` | Alignment helpers and ELF program-header flag mapping are covered. |
| `elfloader/headers.rs`, `info.rs`, `load_base.rs`, `map.rs` | `integration-only` | ELF load planning and interpreter parsing should be unit-tested with in-memory ELF fixtures in a later batch. |
| `process/mod.rs`, `fd_table.rs`, `wait.rs` | `unit-tested` | Default process state, `CLOEXEC` flags, and wait status encoding are covered. |
| `process/execve.rs`, `fork.rs`, `group.rs`, `ptrace.rs` | `integration-only` | Exec/fork/session/ptrace behavior currently couples to global process state. Extract pure helpers before unit testing. |
| `thread/*` | `integration-only` | Thread lifecycle, blocking, scheduling, stacks, and context switching depend on live scheduler and CPU state. |
| `ipc/sysv_shm.rs` | `integration-only` | Global segment registry and process attach state need isolatable helpers before focused unit tests. |
| `object/open_state.rs`, `error.rs`, `bpf.rs`, `memfd.rs`, `file_locks.rs` | `unit-tested` | File flag state, error mapping, BPF array behavior, memfd seal rules, and advisory lock range/conflict/merge helpers are covered. |
| `object/queue_helpers.rs` | `unit-tested` | Queue copy/drain and append ordering are covered; blocking behavior still depends on live scheduler/process signal state. |
| `object/linux_anon.rs`, `netlink.rs` | `integration-only` | Eventfd/timerfd/signalfd/pidfd and netlink state still depend on process/runtime behavior beyond current helper tests. |
| `socket/name.rs`, `inet.rs`, `sockopt.rs` | `unit-tested` | Unix address encoding, inet byte order, and timeout socket-option sizes are covered. |
| `socket/registry.rs` | `unit-tested` | Abstract registry key parsing/ordering is covered in `socket/test.rs` and `registry.rs` tests. Path-backed lookup still depends on VFS/runtime state. |
| `socket/stream.rs`, `datagram.rs`, `pair.rs` | `integration-only` | Local socket state and connection lifecycle still depend on live process/object state. |
| `polling/event.rs` | `unit-tested` | Known and unknown pollable-event conversions are covered. |
| `polling/ready.rs` | `unit-tested` | Ready-event construction is covered in `polling/test.rs`. |
| `polling/entry.rs`, `poller.rs`, `wake.rs` | `integration-only` | Queue wakeups and object registrations still depend on runtime waiters and live object state. |
| `systemcall/numbers.rs`, `table.rs`, `arg_types.rs`, `linux_semantics.rs`, `test_helpers.rs`, `implementations/poll.rs`, `implementations/select.rs` | `unit-tested` | Syscall number lookup, table registration, Linux semantics coverage ledger completeness, typed argument conversion, syscall assertion helpers, poll event/timeout helpers, and select fdset/timeout helpers are covered. |
| `systemcall/implementations/*` syscall bodies | `integration-only` | Syscall bodies generally depend on current process, memory, fd tables, or object side effects. Every registered syscall must have a Linux semantics ledger entry in `systemcall/linux_semantics.rs`; entries marked `CoverageGap` must be converted to `Unit` or `Integration` as behavior tests are added. Boundary flag/timeout helpers should be tested when extracted. |
| `misc/time.rs`, `timer.rs`, `signal.rs`, process exit encoding | `unit-tested` | Time arithmetic, timer state conversion, signal masks, siginfo conversion, and wait encodings are covered. |
| `misc/utsname.rs`, `error.rs`, `stack_builder.rs`, `auxv.rs` | `unit-tested` | Host/domain setters, C-string field layout, kernel-error mapping, stack string/alignment writes, auxv push order, and aux constant values are covered in `misc/test.rs` and internal module tests. |
| `terminal/line_discipline.rs`, `output_filter.rs`, `termios.rs` | `unit-tested` | Canonical/noncanonical input, output CRLF mapping, XTGETTCAP buffering, and default termios basics are covered. |
| `terminal/linux_kd.rs` | `unit-tested` | Linux keyboard/display mode defaults, key-entry tables, and Linux keycode mappings are covered in `terminal/test.rs` and `linux_kd.rs` tests. |
| `terminal/linux_vt.rs`, `object_config.rs`, `object.rs`, `pty/*`, `flanterm/*` | `integration-only` | VT activation/state writes, tty object state, PTY wakeups, and framebuffer terminal output still depend on runtime object/display state. Pure ioctl-number decoding is covered through `object/config.rs` tests. |
| `keyboard/mod.rs` | `unit-tested` | Linux raw scancode prefix remapping is covered. |
| `keyboard/decoding.rs`, `char_processing.rs`, `raw_key_processing.rs`, `key_to_escape_sequence.rs` | `integration-only` | Keyboard decoding and escape mapping should be unit-tested with focused tables in a later input batch. |
| `evdev/device_info.rs`, `event_bits.rs` | `unit-tested` | Device metadata and capability bitmaps are covered. |
| `evdev/ioctl.rs` | `unit-tested` | IOC type/nr/size extraction plus fixed-size/string write helper semantics are covered in `ioctl.rs` tests. |
| `evdev/object.rs`, `queue.rs` | `integration-only` | Queue wakeups, revoke/grab behavior, and client/hub lifecycle still depend on live object state. |
| `drm/mode.rs` | `unit-tested` | DRM fourcc values, default mode constants, and grouped flag unions are covered. |
| `drm/prime.rs` | `unit-tested` | dma-buf ioctl type/nr/dir/size decoding, ioctl name routing, and stable flag bits are covered in `prime.rs` tests. Buffer export/import execution still depends on live DRM/process state. |
| `drm/state.rs`, `client.rs`, `mode_types.rs`, `*_handlers.rs`, `configure.rs` | `integration-only` | Typed request routing is covered indirectly through `object/config.rs` tests, but handler execution still depends on live display/buffer/client state. |
| `net/namespace.rs`, `net/mod.rs` | `integration-only` | Network namespace and interface state are runtime-global today; pure address helpers should be tested as they are added. |
| `drivers/pci`, `drivers/virtio`, `drivers/net/e1000`, `drivers/dma.rs` | `integration-only` | PCI config IO, virtio DMA/HAL behavior, E1000 register programming, and DMA mappings require hardware or emulated devices. |
| `acpi/*` | `integration-only` | ACPI table mapping depends on bootloader memory and physical mapping. |
| `smp/*` | `integration-only` | AP startup, per-CPU state, GS, and topology setup depend on CPU boot sequencing. |
| `interrupts/*` | `integration-only` | IDT/LAPIC/IOAPIC setup and handlers need real interrupt/exception execution. |
| `boot.rs`, `main.rs`, `lib.rs` boot path | `integration-only` | Initialization order is covered by the QEMU integration harness. |
| Module-only `mod.rs` files | `not worth testing` | Re-export and module wiring should stay untested unless behavior is added. |
