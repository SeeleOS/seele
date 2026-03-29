# AGENTS.md

## Overview

SeeleOS is a hobby operating system written primarily in Rust.

The root workspace contains:

- `kernel/`: the OS kernel for `x86_64-unknown-none`
- `seele-sys/`: syscall and shared ABI definitions used by kernel and userspace/libc code
- `packages/`: a small Rust-based ports collection used to fetch, build, and install third-party software into the sysroot
- `relibc-seele/`: the C library and dynamic loader port used by the system
- `toolchain/`: local Rust/LLVM toolchain sources and installer scripts
- `misc/`: runner/build glue for booting the OS image
- `sysroot/`: installed programs, libraries, and runtime assets
- `porting_dev/`: scratch area for package-porting work

The root `Cargo.toml` is a workspace for `seele-sys`, `kernel`, `rust-test`, and `packages`. `toolchain/rust-seele` is intentionally excluded from the workspace.

## Repository Layout

Important directories and their role:

- `kernel/src/systemcall/`: syscall argument decoding and implementations
- `kernel/src/object/`: kernel object model, including tty and poll-related object behavior
- `kernel/src/thread/`: scheduler, blocking, waking, and thread state
- `kernel/src/polling/`: kernel-side poller object and readiness/wake logic
- `kernel/src/misc/time.rs`: kernel time utilities and monotonic/current time helpers
- `packages/src/package/`: individual package recipes
- `packages/<name>/`: package-specific patches and assets
- `relibc-seele/src/platform/seele/`: SeeleOS-specific libc backend
- `relibc-seele/src/header/`: libc header-backed function implementations
- `relibc-seele/src/ld_so/`: dynamic loader code
- `sysroot/programs/`: installed user programs
- `sysroot/misc/`: runtime assets, includes package runtime trees such as Vim runtime files

## Current Focus

Recent commits show active work in these areas:

- kernel time handling and timeout support
- thread blocking and wakeup behavior
- poller and deadline plumbing for `poll`/`select`/`epoll`
- relibc platform work, including dynamic linking and Linux syscall compatibility
- package porting, especially ncurses-based and interactive programs such as Vim

When working in this repository, assume that polling, tty behavior, timeouts, relibc compatibility, and dynamic linking are currently high-risk areas for regressions.

## Build And Verification

Use the narrowest verification that matches the files you touched.

Common commands:

- `cargo check -p kernel`
- `cargo check -p packages`
- `cargo check -p seele-sys`
- `cargo check` from the repository root for workspace-level verification

Package workflow examples:

- `cd packages && cargo run install bash`
- `cd packages && cargo run install busybox`
- `cd packages && cargo run install tinycc`
- `cd packages && cargo run clean <package>`

Toolchain setup lives in `toolchain/README.md`.

If you touch `relibc-seele/`, note that it is its own nested repository with its own build/test flow. Keep root-repo changes and submodule-pointer changes intentional and easy to review.

## Working Rules

- Inspect the relevant subsystem before changing code. Do not guess how kernel objects, syscalls, or relibc backends are wired together.
- Treat `kernel/`, `seele-sys/`, and `relibc-seele/` as an ABI boundary. Changes in one often require matching changes in the others.
- Prefer small, localized changes. Recent work is actively moving core primitives such as time and polling; broad refactors are more likely to break userspace.
- Do not revert unrelated user changes. The repository may legitimately contain in-progress work.
- When working on interactive userspace regressions, check tty semantics, timeout handling, and poller wake logic before blaming package recipes.
- Treat package recipes as the last place to patch around a runtime bug. First verify whether the real issue is in kernel/relibc behavior.

## Polling And TTY Notes

Interactive programs depend on a full stack:

- kernel poller behavior in `kernel/src/polling/`
- thread block/wake logic in `kernel/src/thread/`
- tty behavior in `kernel/src/object/tty_device.rs` and keyboard handling
- relibc `poll`/`select`/`epoll` glue in `relibc-seele/src/platform/seele/` and `relibc-seele/src/header/`

If an interactive program appears to freeze on startup:

- verify timeout semantics first
- verify that blocked threads are removed from the correct queues when woken
- verify ready-state semantics, not just wake delivery
- verify that the relibc-facing timeout type still preserves Linux semantics such as `-1` meaning infinite wait

## Time API Notes

Current kernel time helpers are centered in `kernel/src/misc/time.rs`.

- `Time` is currently modeled as a non-negative nanosecond count
- use monotonic time for deadlines and timeout comparisons
- keep signed timeout values at the syscall boundary long enough to preserve sentinel semantics like `-1`
- convert signed timeout inputs into `Option<deadline>` before blocking threads

## Commit History Guidance

The recent history is useful context and should be checked before modifying active areas. In particular:

- several consecutive commits adjusted thread timeout and polling behavior
- recent relibc commits touched time, uname, umask, and dynamic loader behavior
- package-porting work is ongoing and may expose kernel/libc bugs rather than package-script bugs

Before changing a hot subsystem, review the last several commits for that path and make sure the new change is consistent with the direction already in progress.

## Documentation Expectations

When adding new behavior in hot paths, prefer leaving concise comments where the behavior is non-obvious, especially around:

- timeout semantics
- queue ownership and removal rules
- syscall argument conventions
- ABI assumptions shared with relibc or userspace
