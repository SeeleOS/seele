# Repository Guidelines

## Build, Test, and Development Commands

- `cargo xrun`: launch the VM manually; use `cargo xrun -- --agent` as the serial-driven fallback when the `seele` MCP server is unavailable.
- `cargo xtest`: build and run kernel unit tests in QEMU.
- `cargo xintegration-test`: run integration test coverage.
- `cargo fmt --all`: format Rust code before submitting changes.
- `cargo xrootfs`: build or refresh `disk.img` and the guest root filesystem contents.
- `cargo xrootfs-override`: rebuild `disk.img` from scratch.
- `cargo xsysroot-mount`: mount `sysroot/` from `disk.img` when needed for inspection.
- `cargo xvm-ps`: list current runner and QEMU processes before cleanup.
- `seele` MCP server: when available, prefer its `agent_start`, `agent_status`, `agent_serial_tail`, `agent_screenshot`, QMP input, and cleanup tools for VM-driven agent workflows instead of hand-rolled QMP or terminal-socket scripts.
- If a required tool is missing for this repository workflow, add it to the `flake.nix` dev shell instead of treating it as a one-off host prerequisite.
- When adding new tooling for builds, tests, MCP workflows, debugging, image conversion, or VM automation, prefer adding it to the appropriate `flake.nix` dev shell or runtime input instead of relying on whatever happens to be installed on the host `PATH`.
- When polling VM state or serial output, prefer short polling intervals and frequent checks instead of waiting a long time in one shot.
- After finishing VM-based testing, shut the VM down and verify there is no leftover runner or QEMU process before moving on.
- To inspect the current agent VM and runner processes before shutdown, use `cargo xvm-ps`. If it shows a leftover VM or runner, kill those PIDs explicitly.
- Do not assume `sysroot/` is mounted or synchronized with `disk.img`. Verify whether it is mounted before using it for runtime inspection, and prefer guest logs captured through the xtask VM flow when in doubt.
- If you need `sysroot/` mounted, run `cargo xsysroot-mount` as a separate step first. Do not chain the mount step together with the real inspection command.
- After `cargo xsysroot-mount`, if you only need to read files from `sysroot/`, read them directly without `sudo` or a fresh privilege escalation unless it is actually necessary.
- If the sandbox, `no_new_privileges`, missing mounts, or network restrictions block a necessary command, ask the user for privilege escalation or the required access instead of silently giving up on that path.

After finishing a change, prefer the `seele` MCP workflow when available: run `run_xtest`, then use `agent_start`, `agent_status`, `agent_serial_tail`, and `agent_screenshot` for VM smoke coverage, followed by `agent_stop` or `agent_cleanup`. If MCP is unavailable, run `cargo xtest` and `cargo xrun -- --agent` manually. If any required test fails, keep fixing the issue before considering the work done.

## Coding Style & Naming Conventions

- Prefer `enum` and `bitflags` over integer `const` groups when values are a closed set.
- When a Linux flag set is already modeled as a `bitflags` type, do not duplicate the same bits as separate local `const`s. Reuse the `bitflags` type directly and prefer Linux ABI names such as `MS_*`, `O_*`, or `MAP_*` on the flags themselves.
- Match Linux naming where the kernel exposes Linux ABI behavior.
- Do not write fully qualified type paths inline such as `alloc::string::String`. If a common type is used, import it at the top of the file and use the short name in code.
- When a handle or ID type needs behavior, prefer a dedicated newtype with inherent methods over a `type` alias plus scattered free helper functions.
- Do not accumulate large amounts of unrelated code in one file. Split code by subsystem or feature when a file starts carrying multiple responsibilities, for example moving select-like syscalls into their own `select.rs`.
- When a file grows to cover multiple distinct responsibilities, split it by behavior instead of keeping one catch-all module. Prefer small neighboring modules such as `state.rs`, `events.rs`, `ioctl_display.rs`, or `ioctl_buffer.rs` over monoliths or generic `abi.rs` buckets. DRM-style ABI constants and structs should live next to the subsystem they serve, not in one shared dump file. File size should preferably stay under 200 lines, but it is acceptable to exceed that when the alternative would make the structure worse.
- When there is a clearly better structural solution, prefer it over local patching. In particular, favor changes that remove repetitive boilerplate, unify error handling, and let call sites use direct propagation such as `?` instead of open-coded checks.
- When an existing library or crate feature can cleanly replace handwritten repetitive decoding or boilerplate, prefer using it over custom open-coded conversion logic.
- Do not take shortcuts just to get something running quickly. In particular, avoid adding stubs, temporary shortcuts, or ad-hoc special cases merely to make a feature appear to work.
- If a debug-only stub is temporarily unavoidable, mark it explicitly with `todo!()` or `unimplemented!()`. If it cannot use either, add a clear `TODO` comment stating that it is a temporary debug stub and not a real implementation.
- For syscall handlers, do not take a user pointer as `u64` and then immediately cast it to `*const T` or `*mut T` in the body. Make the syscall argument itself a properly typed pointer and add or reuse the `SyscallArg` conversion in `kernel/src/systemcall/arg_types.rs`.
- For syscall flag arguments and similar closed ABI bitfields, do not manually call `from_bits*()` inside syscall bodies or pass raw integers through internal helpers when a typed flag would do. Convert at the syscall boundary with `SyscallArg`, make syscall parameters strongly typed, and have helper functions take the typed flag directly unless there is a clear special-case reason not to.
- For Linux ioctls, prefer adding explicit `ConfigurateRequest` variants and decoding them at the ioctl boundary instead of matching raw ioctl numbers inside device implementations. Treat `RawIoctl` as a last resort passthrough path, not the default way to add tty/ioctl support.

## Testing Guidelines

There is no large standalone test suite yet; verification is primarily compile checks plus QEMU boot tests.

- IMPORTANT: Syscall tests and other ABI-facing tests must not stop at a minimal smoke test. They must cover Linux-standard flag combinations, errno behavior, Linux-specific edge cases, structure/layout expectations, state side effects, and the actual Linux semantics required for binary compatibility.
- IMPORTANT: Do not treat ledger coverage, one happy-path call, or return-value-only assertions as sufficient ABI coverage. When adding or changing syscall, ioctl, device, filesystem, socket, terminal, memory-mapping, or graphics behavior, tests must exercise the full externally visible side effects and realistic edge cases for that ABI: mutated kernel state, data copied to or from user buffers, fd/object flags, queues and wakeups, mapped-memory visibility, framebuffer/device contents, partial/truncated buffers, invalid pointers, invalid flags, boundary sizes, repeated calls, and error ordering where Linux defines it.
- This kernel targets Linux binary compatibility. Syscall tests and other ABI-facing tests must use Linux semantics as the only oracle, not the current implementation behavior. Validate x86_64 Linux syscall ABI return values, errno values, struct layouts, flag combinations, state side effects, and Linux-specific edge cases. If implementation behavior disagrees with Linux semantics, fix the kernel implementation instead of relaxing tests to accept the wrong behavior.
- Run `cargo check --manifest-path kernel/Cargo.toml` for all kernel changes.
- Run `cargo xtest` for kernel unit-test coverage.
- Treat compiler warnings as failures. Do not leave any `cargo check` warnings in the tree.
- After finishing code changes, run `cargo clippy` and address its findings before considering the work complete.
- For syscall, process, terminal, or userspace changes, prefer the MCP agent VM path when available. Use `cargo xrun -- --agent` as the manual fallback.
- When validating MCP-driven VM behavior, verify QMP connectivity, serial log output, and screenshot capture when relevant, then stop the VM through the MCP cleanup/stop tool or by killing the reported runner and QEMU PIDs.
- Add focused unit tests only when the target module already uses them.

## Debugging Guidance

IMPORTANT: When debugging third-party userspace components such as Weston, Xorg, libudev, libinput, KDE, Qt, or SQLite, do not rely on staring at binaries or disassembly unless there is no better option. Prefer reading the corresponding source code first.
IMPORTANT: When debugging third-party source code in this repository workflow, inspect the official upstream GitHub repository first and treat binary inspection or disassembly as a last resort only after the source path has been exhausted.
If GitHub is unavailable, or if you need a local checkout for broad `rg` searches, clone the relevant upstream repository into a clearly named local directory such as `third_party/`, then use that local checkout as a secondary reference.
If you need syscall-level debugging, temporarily enable `should_log` in `kernel/src/systemcall/handling.rs` manually, and turn it back off before finishing the task.
When syscall logging is needed to chase userspace failures, prefer filtering the log to syscalls that return a specific errno of interest such as `BadAddress` instead of logging every syscall entry/exit. This keeps `mmap`, `read`, `write`, `poll`, and `futex` noise from hiding the actual signal.
If the system appears to stop responding, consider early that a syscall may have entered the kernel and never returned. Use enter/exit syscall logs to verify this explicitly instead of assuming the last logged successful syscall was the true point of failure.
IMPORTANT: If the system appears to stop making progress without an obvious crash, treat deadlock or lock re-entry as a primary suspect early instead of assuming the problem is only scheduler starvation or missing syscalls.
If the system appears unresponsive, try typing into it and checking for visible echo or other reaction, while keeping in mind that some software may have intentionally disabled echo.
If temporary debug output is needed in kernel code, use `s_println!` for those ad-hoc debug messages instead of `log::info!` or plain `print`-style output.
IMPORTANT: After the root cause is confirmed, only keep permanent fixes. If the issue is in a kernel syscall or ABI path, fix the kernel gap. If it is in rootfs packaging, runtime environment, component presence, or permissions, fix the build or image contents instead.
IMPORTANT: Do not accept fake fixes such as swapping themes, seeding fake backgrounds, or keeping long-term debug wrappers just to make the screen look less broken.
IMPORTANT: After debugging is done, remove any temporary debug logs, extra serial prints, or ad-hoc instrumentation you added during investigation.
IMPORTANT: After debugging is done, restore the normal boot path and remove temporary syscall logs, serial prints, wrappers, and extra rootfs debug overlays.
If temporary runtime logging grows noisy enough to hide the actual signal, narrow or remove the unhelpful logs instead of letting large traces accumulate.
If the current logs are already noisy enough to pollute the debugging signal and a given log is no longer necessary, clean it up promptly instead of keeping it around.
Do not use `strace` inside the VM for guest userspace debugging in this repository. The current environment has known issues around `strace` behavior that make it a poor debugging tool here. Prefer targeted kernel syscall logging or other focused instrumentation instead.

## Repository Layout Notes

- `rootfs_making/` contains the disk image construction script and the flat set of guest rootfs overlay/config files that `cargo xrootfs` installs into `sysroot/`.

## Commit & Pull Request Guidelines

Recent commits are short, imperative, and lowercase, for example: `deleted seele-sys fully` or `linux stuff`.

- IMPORTANT: split commits by feature/fix.
- IMPORTANT: after finishing a discrete feature or fix, commit it immediately even before verification. If verification later finds a problem, fix that in a separate follow-up commit instead of delaying the original commit.
- IMPORTANT: make small verified commits promptly while debugging.
- IMPORTANT: once a discrete feature or fix is verified, commit it immediately instead of waiting for the rest of the work to finish.
- IMPORTANT: before committing, review the current `git diff` against `AGENTS.md`, then split and commit by feature/fix.
- Do not let multiple unrelated runtime experiments, partial fixes, or cleanup work accumulate in one uncommitted batch.
- Keep commit titles concise and action-oriented.
- One logical change per commit when practical.
- After completing a discrete feature or fix and verifying it, make a git commit for that completed work instead of leaving it uncommitted.
- For small, focused fixes, make a dedicated commit immediately after the change is verified instead of batching it with later unrelated work.
- Do not let large batches of unrelated or only partially separated changes accumulate uncommitted. Prefer committing each small verified step promptly while debugging.
- PRs should explain the behavior change, affected subsystems, and exact verification steps.
- Include serial log excerpts or screenshots when changing boot, terminal, or shell behavior.

## Collaboration Notes

- Speak with the user in Chinese.
- If the user provides a workflow or debugging suggestion that is broadly useful for future work in this repository, add it to `AGENTS.md` when appropriate instead of treating it as a one-off remark.
- For Codex-driven VM interaction, prefer the `seele` MCP server when it is available. Use `agent_start` to launch the VM, `agent_status` to confirm the runner/QEMU/QMP state, `agent_serial_tail` for boot logs, `agent_screenshot` for the display, and the QMP key/mouse tools for guest input.
- Do not reintroduce the old background terminal input workflow. `cargo xrun -- --agent` no longer prints or uses a `background terminal input path`, `/tmp/seele-agent-tty.sock`, `SEELE_AGENT_TTY_SOCKET`, or a second guest serial port for stdin forwarding.
- For manual fallback when MCP is unavailable, use `cargo xrun -- --agent` for serial-driven boot verification and `cargo xvm-ps` plus host-side `kill` for cleanup. Do not invent a tty-socket or ad-hoc input path.
- When debugging interactive login issues where the user needs to type a username or password manually, run `nix develop -c cargo xrun` in the foreground and let the user provide the login input, or use MCP QMP input if the login can be driven programmatically.
- If QMP input appears to produce no reaction, treat kernel deadlock, echo being disabled, or the foreground being owned by another program as primary explanations to check before blaming the MCP transport.
- After you finish using an MCP, interactive, or agent VM, terminate it yourself instead of relying on a default runner timeout to clean it up.
- When terminating a leftover agent VM, first run `cargo xvm-ps`, then `kill` the reported runner and QEMU PIDs. If the VM was started by the `seele` MCP server, prefer `agent_stop` or `agent_cleanup` first.
- Do not rely on guest-side `poweroff` in this environment. If you need to stop the agent VM, inspect it with `cargo xvm-ps` and `kill` the reported runner and QEMU PIDs from the host side, or use MCP cleanup for MCP-managed sessions.
- If `sysroot/` already appears to be mounted, reuse it directly instead of asking for privilege escalation to mount again. Only ask to mount when it is clearly not mounted.
- When you need to mount `sysroot/`, use `cargo xsysroot-mount` directly. Run it first, then run the real inspection command separately instead of chaining them together.
