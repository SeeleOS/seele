# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace for a small x86_64 OS.

- `kernel/`: the kernel crate. Most development happens in `kernel/src/`.
- `kernel/src/systemcall/`: syscall numbers, dispatch, and implementations.
- `kernel/src/filesystem/`, `memory/`, `process/`, `thread/`, `terminal/`: core kernel subsystems.
- `misc/runner.rs`: QEMU runner used by `cargo run`.
- `scripts/`: helper scripts.
- `disk.img` and `sysroot/`: guest root filesystem image and mounted contents.

Keep new code close to the subsystem it belongs to. For example, terminal ioctls belong under `kernel/src/terminal/`, not in a generic ABI file.

## Build, Test, and Development Commands

- `cargo check --manifest-path kernel/Cargo.toml`: fast kernel-only compile check.
- `cargo check`: compile the workspace, including the runner.
- `cargo run`: build and boot the OS in QEMU.
- `nix develop`: enter the project dev shell.
- `nix develop -c cargo run -- --agent`: headless VM run with timeout and serial output; use this for runtime verification.
- `cargo fmt --all`: format Rust code before submitting changes.
- When launching the VM during agent work, use a checked-in `.sh` wrapper script instead of invoking the VM command directly. Put any needed log redirection inside the wrapper rather than on the outer command line.
- When using the checked-in VM wrapper, run it directly (for example `misc/run-agent-vm.sh`). Do not wrap it with `bash`, and do not override its default log file path unless explicitly requested.
- Do not assume `sysroot/` is mounted or synchronized with `disk.img`. Verify whether it is mounted before using it for runtime inspection, and prefer guest logs captured through the VM wrapper when in doubt.
- If the sandbox, `no_new_privileges`, missing mounts, or network restrictions block a necessary command, ask the user for privilege escalation or the required access instead of silently giving up on that path.

After finishing a change, run `nix develop -c cargo run -- --agent` to test the VM. If the VM test fails, keep fixing the issue before considering the work done. If you are validating a shell or userspace fix, prefer the `--agent` path so serial logs are captured automatically.

## Coding Style & Naming Conventions

- Use Rust 2024 style and keep formatting `rustfmt`-clean.
- Indent with 4 spaces; do not use tabs for new code.
- Prefer `enum` and `bitflags` over integer `const` groups when values are a closed set.
- Use `snake_case` for functions/modules, `CamelCase` for types, and short descriptive names for syscalls and kernel objects.
- Match Linux naming where the kernel exposes Linux ABI behavior.
- Do not accumulate large amounts of unrelated code in one file. Split code by subsystem or feature when a file starts carrying multiple responsibilities, for example moving select-like syscalls into their own `select.rs`.
- When there is a clearly better structural solution, prefer it over local patching. In particular, favor changes that remove repetitive boilerplate, unify error handling, and let call sites use direct propagation such as `?` instead of open-coded checks.
- Do not take shortcuts just to get something running quickly. In particular, avoid adding stubs, temporary shortcuts, or ad-hoc special cases merely to make a feature appear to work.
- For syscall handlers, do not take a user pointer as `u64` and then immediately cast it to `*const T` or `*mut T` in the body. Make the syscall argument itself a properly typed pointer and add or reuse the `SyscallArg` conversion in `kernel/src/systemcall/arg_types.rs`.

## Testing Guidelines

There is no large standalone test suite yet; verification is primarily compile checks plus QEMU boot tests.

- Run `cargo check --manifest-path kernel/Cargo.toml` for all kernel changes.
- Run `nix develop -c cargo run -- --agent` for syscall, process, terminal, or userspace changes.
- Add focused unit tests only when the target module already uses them.

## Debugging Guidance

If you hit an unimplemented syscall or similar runtime gap, check `../elysia-os` and `../elysia-os/relibc` first before designing a new solution. In many cases there is already a working implementation or a compatible approach there that should be reused or mirrored here.
Treat `../elysia-os` and `../elysia-os/relibc` as reference code only. Do not assume this repository is currently using `relibc` or the exact same userspace stack; verify against the actual packages and binaries present in the current image.
When debugging third-party userspace components such as Xorg, libudev, or libinput, do not rely on staring at binaries or disassembly unless there is no better option. Prefer reading the corresponding source code first.
If the relevant source tree is not already present locally, do not have the agent clone it directly. The agent's network access is unreliable for this repository workflow. Instead, tell the user exactly which upstream or packaged source tree to clone into a clearly named local directory such as `third_party/`, then use that local checkout as the primary reference during debugging.
When debugging third-party source code in this repository workflow, do not use web search as the primary way to inspect source. Clone the upstream repository into a local `third_party/` directory and use that local checkout instead.
If you need syscall-level debugging, temporarily enable `should_log` in `kernel/src/systemcall/handling.rs` manually, and turn it back off before finishing the task.
When syscall logging is needed to chase userspace failures, prefer filtering the log to syscalls that return a specific errno of interest such as `BadAddress` instead of logging every syscall entry/exit. This keeps `mmap`, `read`, `write`, `poll`, and `futex` noise from hiding the actual signal.
After debugging is done, remove any temporary debug logs, extra serial prints, or ad-hoc instrumentation you added during investigation.
If temporary runtime logging grows noisy enough to hide the actual signal, narrow or remove the unhelpful logs instead of letting large traces accumulate.

## Commit & Pull Request Guidelines

Recent commits are short, imperative, and lowercase, for example: `deleted seele-sys fully` or `linux stuff`.

- Keep commit titles concise and action-oriented.
- One logical change per commit when practical.
- For small, focused fixes, make a dedicated commit immediately after the change is verified instead of batching it with later unrelated work.
- PRs should explain the behavior change, affected subsystems, and exact verification steps.
- Include serial log excerpts or screenshots when changing boot, terminal, or shell behavior.

## Collaboration Notes

- If the user provides a workflow or debugging suggestion that is broadly useful for future work in this repository, add it to `AGENTS.md` when appropriate instead of treating it as a one-off remark.
