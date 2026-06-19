# Repository Guidelines

## Build, Test, and Development Commands

- `cargo xrun`, `cargo xtest`, `cargo xbuild-rootfs`, and related build/test commands are MCP-driven workflows in this repository. Do not run them directly on the host shell when MCP is available.
- If MCP does not yet expose the needed action, extend the `seele` MCP server first instead of inventing a one-off host command path.
- `cargo fmt --all`: format Rust code before submitting changes.
- Prefer the `seele` MCP server for VM, serial, screenshot, input, cleanup, build, and test workflows. Do not run ad-hoc bare host commands for those tasks when MCP is available.
- `seele` MCP server: when available, prefer its `start`, `status`, `serial_tail`, `screenshot`, QMP input, cleanup, and test/build tools instead of hand-rolled QMP, terminal-socket scripts, or direct host cargo invocations.
- If a required tool is missing for this repository workflow, add it to the `flake.nix` dev shell instead of treating it as a one-off host prerequisite.
- When adding new tooling for builds, tests, MCP workflows, debugging, image conversion, or VM automation, prefer adding it to the appropriate `flake.nix` dev shell or runtime input instead of relying on whatever happens to be installed on the host `PATH`.
- When polling VM state or serial output, prefer short polling intervals and frequent checks instead of waiting a long time in one shot.
- `cargo xtest` and MCP `run_tests` default to kernel unit tests plus LTP. Use `cargo xtest full` or MCP `run_tests(test: "full")` when every integration test is required, including boot/image/panic smoke coverage. Use a specific filter such as `ltp` or `integration::panic_handler_smoke` for targeted debugging.
- After finishing VM-based testing, shut the VM down and verify there is no leftover runner or QEMU process before moving on.
- To inspect the current VM and runner processes before shutdown, prefer `status` for MCP-managed sessions; otherwise inspect host runner/QEMU processes directly and kill leftover PIDs explicitly.
- Do not assume `target/rootfs_mnt/` is mounted or synchronized with `target/rootfs.img`. Verify whether it is mounted before using it for runtime inspection, and prefer guest logs captured through the xtask VM flow when in doubt.
- If you need `target/rootfs_mnt/` mounted, prefer the MCP `ensure_rootfs_mounted` tool; for manual fallback, check `mountpoint -q target/rootfs_mnt` and mount `target/rootfs.img` to `target/rootfs_mnt/` as a separate step first. Do not chain the mount step together with the real inspection command.
- After mounting `target/rootfs_mnt/`, if you only need to read files from it, read them directly without `sudo` or a fresh privilege escalation unless it is actually necessary.
- If the sandbox, `no_new_privileges`, missing mounts, or network restrictions block a necessary command, ask the user for privilege escalation or the required access instead of silently giving up on that path.

After finishing a change, prefer the `seele` MCP workflow when available: run `run_tests` for the default kernel-unit-plus-LTP gate. For boot, terminal, panic, image, or VM-launch changes, run `run_tests(test: "full")` or the relevant targeted integration test, then use `start`, `status`, `serial_tail`, and `screenshot` for VM smoke coverage, followed by `stop` or `cleanup`. If MCP is missing a required capability, add that capability to MCP rather than falling back to direct host project commands. If any required test fails, keep fixing the issue before considering the work done.

## Coding Style & Naming Conventions

- Prefer `enum` and `bitflags` over integer `const` groups when values are a closed set.
- Prefer modern Rust and the latest stable idioms; use nightly-only features when they materially improve correctness, clarity, or maintainability.
- When a Linux flag set is already modeled as a `bitflags` type, do not duplicate the same bits as separate local `const`s. Reuse the `bitflags` type directly and prefer Linux ABI names such as `MS_*`, `O_*`, or `MAP_*` on the flags themselves.
- Match Linux naming where the kernel exposes Linux ABI behavior.
- Do not write fully qualified type paths inline such as `alloc::string::String`. If a common type is used, import it at the top of the file and use the short name in code.
- When a handle or ID type needs behavior, prefer a dedicated newtype with inherent methods over a `type` alias plus scattered free helper functions.
- Do not accumulate large amounts of unrelated code in one file. Split code by subsystem or feature when a file starts carrying multiple responsibilities, for example moving select-like syscalls into their own `select.rs`.
- When a file grows to cover multiple distinct responsibilities, split it by behavior instead of keeping one catch-all module. Prefer small neighboring modules such as `state.rs`, `events.rs`, `ioctl_display.rs`, or `ioctl_buffer.rs` over monoliths or generic `abi.rs` buckets. DRM-style ABI constants and structs should live next to the subsystem they serve, not in one shared dump file. File size should preferably stay under 200 lines, but it is acceptable to exceed that when the alternative would make the structure worse.
- When there is a clearly better structural solution, prefer it over local patching. In particular, favor changes that remove repetitive boilerplate, unify error handling, and let call sites use direct propagation such as `?` instead of open-coded checks.
- When an existing library or crate feature can cleanly replace handwritten repetitive decoding or boilerplate, prefer using it over custom open-coded conversion logic.
- Prefer existing crates over reinventing well-covered functionality locally.
- Prefer `Into` conversions over `From` at call sites. If inference makes the code unclear, use an explicit target type with `let value: Target = source.into();` or `into::<Target>()`.
- Avoid turbofish syntax when a clear local type annotation lets the compiler infer the generic type naturally.
- Prefer macro-based abstractions when they remove meaningful repetition without hiding important control flow or making diagnostics worse.
- Prefer chain-style Rust APIs when they stay readable and do not obscure error handling or ownership.
- Do not take shortcuts just to get something running quickly. In particular, avoid adding stubs, temporary shortcuts, or ad-hoc special cases merely to make a feature appear to work.
- During refactors, do not leave compatibility layers, fallback paths, or old/new dual implementations behind. Update call sites to the new structure directly and remove the obsolete path in the same change unless there is an explicit migration plan.
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
- Run `cargo xtest` for the default kernel unit-test plus LTP coverage.
- Run `cargo xtest full` before broad integration submissions or when touching boot/image/panic/VM launch behavior.
- Treat compiler warnings as failures. Do not leave any `cargo check` warnings in the tree.
- After finishing code changes, run `cargo clippy` and address its findings before considering the work complete.
- Put focused Rust unit tests in the same file as the code they cover, inside a local `#[cfg(test)] mod tests {}` block. Do not create separate `test.rs` files for new code unless the existing module already uses that layout or the tests must span multiple files.
- For syscall, process, terminal, or userspace changes, prefer the MCP VM path when available. Do not use `cargo xrun -- --agent` as a manual fallback; add the missing MCP capability instead.
- When validating MCP-driven VM behavior, verify QMP connectivity, serial log output, and screenshot capture when relevant, then stop the VM through the MCP cleanup/stop tool or by killing the reported runner and QEMU PIDs.
- Add focused unit tests only when the target module already uses them.

## Debugging Guidance

IMPORTANT: When debugging third-party userspace components such as Weston, Xorg, libudev, libinput, KDE, Qt, or SQLite, do not rely on staring at binaries or disassembly unless there is no better option. Prefer reading the corresponding source code first.
IMPORTANT: When debugging third-party source code in this repository workflow, inspect the official upstream GitHub repository first and treat binary inspection or disassembly as a last resort only after the source path has been exhausted.
If GitHub is unavailable, or if you need a local checkout for broad `rg` searches, clone the relevant upstream repository into a clearly named local directory such as `third_party/`, then use that local checkout as a secondary reference.
Do not clone upstream source when MCP or direct fetch access is available; prefer reading it through MCP/fetch first and only clone as a last resort when those paths are blocked.
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

- `xtask/src/build_rootfs/` contains the rootfs image and guest rootfs construction workflow used by `cargo xbuild-rootfs`.

## Review Terminology

When doing a code review for this repository, use these review skills together unless the user explicitly narrows the scope: `review-agents-md-adherence`, `review-maintainability`, and `review-simplicity`.

When the user asks to review "屎山代码" or "史山代码", interpret that as a maintainability and architecture review focused on these failure modes:

- 结构混乱：模块边界不清，一个文件或函数承担太多职责。
- 依赖纠缠：改 A 会莫名影响 B，调用链和状态流很难追。
- 重复逻辑多：同一套判断、转换、错误处理复制很多份，修 bug 容易漏。
- 隐式行为多：靠全局状态、副作用、魔法数字、特殊约定工作。
- 命名差：变量、函数、模块名无法表达真实意图。
- 抽象不对：要么没有抽象，到处复制；要么过度抽象，读代码像解谜。
- 错误处理随意：吞错误、乱返回默认值、panic/unwrap 滥用，失败路径没人懂。
- 测试薄弱：只有 happy path，边界条件、错误语义、状态副作用没覆盖。
- 临时补丁长期化：TODO、hack、stub、特殊 case 留在主路径里。
- 格式和风格不一致：不同人、不同时间写出来的代码像几个项目拼在一起。
- 改动成本高：一个小需求需要碰很多地方，而且没人敢确定影响范围。

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
- For Codex-driven VM interaction, prefer the `seele` MCP server when it is available. Use `start` to launch the VM, `status` to confirm the runner/QEMU/QMP state, `serial_tail` for boot logs, `screenshot` for the display, and the QMP key/mouse tools for guest input.
- When MCP is available, do not replace these workflows with ad-hoc bare shell commands; use the MCP tools first and only fall back to manual commands when MCP is unavailable.
- Do not reintroduce the old background terminal input workflow. `cargo xrun -- --agent` no longer prints or uses a `background terminal input path`, `/tmp/seele-agent-tty.sock`, `SEELE_AGENT_TTY_SOCKET`, or a second guest serial port for stdin forwarding.
- Do not rely on direct host-side cargo/QEMU/manual process workflows as a fallback. If MCP cannot do the job, extend MCP and use that path instead.
- When debugging interactive login issues where the user needs to type a username or password manually, run `nix develop -c cargo xrun` in the foreground and let the user provide the login input, or use MCP QMP input if the login can be driven programmatically.
- If QMP input appears to produce no reaction, treat kernel deadlock, echo being disabled, or the foreground being owned by another program as primary explanations to check before blaming the MCP transport.
- After you finish using an MCP, interactive, or VM, terminate it yourself instead of relying on a default runner timeout to clean it up.
- When terminating a leftover VM, first inspect the current runner and QEMU processes, then `kill` the reported runner and QEMU PIDs. If the VM was started by the `seele` MCP server, prefer `stop` or `cleanup` first.
- Do not rely on guest-side `poweroff` in this environment. If you need to stop the VM, inspect runner and QEMU PIDs from the host side and kill them, or use MCP cleanup for MCP-managed sessions.
- If `target/rootfs_mnt/` already appears to be mounted, reuse it directly instead of asking for privilege escalation to mount again. Only ask to mount when it is clearly not mounted.
- When you need to mount `target/rootfs_mnt/`, prefer MCP `ensure_rootfs_mounted`; for manual fallback, inspect `mountpoint -q target/rootfs_mnt` and mount `target/rootfs.img` to `target/rootfs_mnt/` separately before the real inspection command.
