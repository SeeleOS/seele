# Repository Guidelines

## Build, Test, and Development Commands

- `nix develop -c cargo run -- --agent`: headless VM run with timeout and serial output; use this for runtime verification.
- `cargo fmt --all`: format Rust code before submitting changes.
- `rootfs_making/make_disk.sh`: build or refresh `disk.img` and the guest root filesystem contents.
- When launching the VM during agent work, use a checked-in `.sh` wrapper script instead of invoking the VM command directly. Put any needed log redirection inside the wrapper rather than on the outer command line.
- When using the checked-in VM wrapper, run it directly (for example `misc/run-agent-vm.sh`). Do not wrap it with `bash`, and do not override its default log file path unless explicitly requested.
- `misc/run-agent-vm.sh` should not impose a default timeout. If a timeout is needed for a specific debugging pass, set it explicitly and shut the VM down yourself when finished.
- When polling a background VM terminal, prefer short polling intervals and frequent checks instead of waiting a long time in one shot.
- After finishing VM-based testing, shut the VM down and verify there is no leftover background runner or QEMU process before moving on.
- Do not assume `sysroot/` is mounted or synchronized with `disk.img`. Verify whether it is mounted before using it for runtime inspection, and prefer guest logs captured through the VM wrapper when in doubt.
- If the sandbox, `no_new_privileges`, missing mounts, or network restrictions block a necessary command, ask the user for privilege escalation or the required access instead of silently giving up on that path.

After finishing a change, run `nix develop -c cargo run -- --agent` to test the VM. If the VM test fails, keep fixing the issue before considering the work done. If you are validating a shell or userspace fix, prefer the `--agent` path so serial logs are captured automatically.

## Coding Style & Naming Conventions

- Prefer `enum` and `bitflags` over integer `const` groups when values are a closed set.
- When a Linux flag set is already modeled as a `bitflags` type, do not duplicate the same bits as separate local `const`s. Reuse the `bitflags` type directly and prefer Linux ABI names such as `MS_*`, `O_*`, or `MAP_*` on the flags themselves.
- Match Linux naming where the kernel exposes Linux ABI behavior.
- Do not write fully qualified type paths inline such as `alloc::string::String`. If a common type is used, import it at the top of the file and use the short name in code.
- When a handle or ID type needs behavior, prefer a dedicated newtype with inherent methods over a `type` alias plus scattered free helper functions.
- Do not accumulate large amounts of unrelated code in one file. Split code by subsystem or feature when a file starts carrying multiple responsibilities, for example moving select-like syscalls into their own `select.rs`.
- When there is a clearly better structural solution, prefer it over local patching. In particular, favor changes that remove repetitive boilerplate, unify error handling, and let call sites use direct propagation such as `?` instead of open-coded checks.
- When an existing library or crate feature can cleanly replace handwritten repetitive decoding or boilerplate, prefer using it over custom open-coded conversion logic.
- Do not take shortcuts just to get something running quickly. In particular, avoid adding stubs, temporary shortcuts, or ad-hoc special cases merely to make a feature appear to work.
- For syscall handlers, do not take a user pointer as `u64` and then immediately cast it to `*const T` or `*mut T` in the body. Make the syscall argument itself a properly typed pointer and add or reuse the `SyscallArg` conversion in `kernel/src/systemcall/arg_types.rs`.
- For syscall flag arguments and similar closed ABI bitfields, do not manually call `from_bits*()` inside syscall bodies or pass raw integers through internal helpers when a typed flag would do. Convert at the syscall boundary with `SyscallArg`, make syscall parameters strongly typed, and have helper functions take the typed flag directly unless there is a clear special-case reason not to.
- For Linux ioctls, prefer adding explicit `ConfigurateRequest` variants and decoding them at the ioctl boundary instead of matching raw ioctl numbers inside device implementations. Treat `RawIoctl` as a last resort passthrough path, not the default way to add tty/ioctl support.

## Testing Guidelines

There is no large standalone test suite yet; verification is primarily compile checks plus QEMU boot tests.

- Run `cargo check --manifest-path kernel/Cargo.toml` for all kernel changes.
- Treat compiler warnings as failures. Do not leave any `cargo check` warnings in the tree.
- After finishing code changes, run `cargo clippy` and address its findings before considering the work complete.
- Run `nix develop -c cargo run -- --agent` for syscall, process, terminal, or userspace changes.
- Add focused unit tests only when the target module already uses them.

## Debugging Guidance

When debugging third-party userspace components such as Xorg, libudev, or libinput, do not rely on staring at binaries or disassembly unless there is no better option. Prefer reading the corresponding source code first.
If the relevant source tree is not already present locally, do not have the agent clone it directly. The agent's network access is unreliable for this repository workflow. Instead, tell the user exactly which upstream or packaged source tree to clone into a clearly named local directory such as `third_party/`, then use that local checkout as the primary reference during debugging.
When debugging third-party source code in this repository workflow, do not use web search as the primary way to inspect source. Clone the upstream repository into a local `third_party/` directory and use that local checkout instead.
If you need syscall-level debugging, temporarily enable `should_log` in `kernel/src/systemcall/handling.rs` manually, and turn it back off before finishing the task.
When syscall logging is needed to chase userspace failures, prefer filtering the log to syscalls that return a specific errno of interest such as `BadAddress` instead of logging every syscall entry/exit. This keeps `mmap`, `read`, `write`, `poll`, and `futex` noise from hiding the actual signal.
If the system appears to stop responding, consider early that a syscall may have entered the kernel and never returned. Use enter/exit syscall logs to verify this explicitly instead of assuming the last logged successful syscall was the true point of failure.
If the system appears to stop making progress without an obvious crash, treat deadlock or lock re-entry as a primary suspect early instead of assuming the problem is only scheduler starvation or missing syscalls.
If temporary debug output is needed in kernel code, use `s_println!` for those ad-hoc debug messages instead of `log::info!` or plain `print`-style output.
After debugging is done, remove any temporary debug logs, extra serial prints, or ad-hoc instrumentation you added during investigation.
If temporary runtime logging grows noisy enough to hide the actual signal, narrow or remove the unhelpful logs instead of letting large traces accumulate.
If the current logs are already noisy enough to pollute the debugging signal and a given log is no longer necessary, clean it up promptly instead of keeping it around.

## Repository Layout Notes

- `rootfs_making/` contains the disk image construction script and the flat set of guest rootfs overlay/config files that `make_disk.sh` installs into `sysroot/`.

## Commit & Pull Request Guidelines

Recent commits are short, imperative, and lowercase, for example: `deleted seele-sys fully` or `linux stuff`.

- IMPORTANT: split commits by feature/fix.
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

- If the user provides a workflow or debugging suggestion that is broadly useful for future work in this repository, add it to `AGENTS.md` when appropriate instead of treating it as a one-off remark.
- When debugging interactive login issues where the user needs to type a username or password manually, run `nix develop -c cargo run` in the foreground instead of the `--agent` path and let the user provide the login input.
- When a background VM terminal is available, prefer interacting with it directly to send guest tty input, including login credentials and shell commands, instead of routing that input through helper scripts or separate tty socket tooling.
- If a background VM terminal is available, interact with that terminal session itself for guest input. Do not fall back to tty sockets, `nc`, or similar side channels when direct terminal interaction is possible.
- `run agent vm` should be treated as directly interactive by default. Do not assume a separate tty socket or extra terminal wrapper is needed just to type into the guest.
- After you finish using an interactive or background VM, terminate it yourself instead of relying on a default runner timeout to clean it up.
- If `sysroot/` already appears to be mounted, reuse it directly instead of asking for privilege escalation to mount again. Only ask to mount when it is clearly not mounted.
