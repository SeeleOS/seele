# Seele OS

Minimal build instructions.

## Prerequisites

- Linux host
- `git`
- `nix` with flakes enabled
- `qemu-system-x86_64`
- `sudo` access for mounting/unmounting `disk.img`

Clone submodules first:

```sh
git submodule update --init --recursive
```

## Install Nix

If you do not already have Nix installed, run:

```sh
sh <(curl -L https://nixos.org/nix/install) --daemon
```

Then restart your shell and enable flakes:

```sh
mkdir -p ~/.config/nix
cat > ~/.config/nix/nix.conf <<'EOF'
experimental-features = nix-command flakes
EOF
```

## Run directly

From the repository root, you can build and boot the OS directly with:

```sh
nix run
```

This uses the Rust toolchain pinned in [rust-toolchain.toml](/home/elysia/coding-project/seele-os-linux/rust-toolchain.toml) through the flake, builds the runner and kernel, and launches QEMU.

## Enter the dev shell

From the repository root:

```sh
nix develop
```

## Install the local Rust toolchain

The project expects a local Rust toolchain named `seele`:

```sh
cd toolchain
./install.rs
cd ..
```

## Populate the sysroot

Install at least the packages you want inside the disk image:

```sh
cd packages
cargo run install busybox
cargo run install bash
cargo run install tinycc
cd ..
```

This writes into the mounted `sysroot` directory backed by `disk.img`.

## Build and run with Cargo

From the repository root:

```sh
cargo run
```

This builds the kernel, creates a bootable image, and launches QEMU.

## Notes

- `cargo run` unmounts `sysroot` before building.
- The runner uses UEFI QEMU boot by default.
- If `/dev/kvm` exists, QEMU will use KVM automatically.
