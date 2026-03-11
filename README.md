# Elysia OS

A small hobby OS written in Rust with a custom kernel, basic userspace, and a growing POSIX-compat layer.

## Features

- x86_64 kernel with userland support
- Basic process model (fork/exec/exit/wait)
- Virtual filesystem with FAT32 backend
- TTY/terminal rendering on framebuffer
- Custom libc work-in-progress (relibc)
- Syscall layer and userspace syslib

## Screenshots

> TODO: add screenshot

## Build and Run

Prereqs:
- Rust toolchain with `x86_64-unknown-none` target
- QEMU (`qemu-system-x86_64`)

Optional (for userspace C programs):
- `x86_64-elf-gcc`

Build and run (UEFI or BIOS):

```bash
cargo run --bin elysia-os-runner --release -- uefi
# or
cargo run --bin elysia-os-runner --release -- bios
```

Build sample userspace programs:

```bash
make -C Manuae-Shell
make -C posix-tests
```

## Usage

- Boot the OS with QEMU (see Build and Run).
- The default shell is loaded from `/programs` in the sysroot image.
- Use `posix-tests.elf` to validate syscalls and basic POSIX behavior.

## Contributing

I want this project to grow into something real. If you are interested, please contribute — even small changes help a lot.

I’m actively trying to grow this project and would love more contributors. If you hit any bugs or have questions, please open an issue so we can track and discuss it.

- If you find a bug or have a feature idea, open an issue.
- If you want to help, pick something in `todo-list.md` and send a PR.
- Feel free to ask questions or propose design changes; I want to keep improving the OS with community input.

## Notes

This is a hobby OS in active development. Expect breaking changes and rough edges.
