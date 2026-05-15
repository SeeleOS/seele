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

From the repository root:

```sh
nix run
```

This enters the flake environment, initializes the rootfs if needed, and runs `cargo xrun`.

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

## Build and run with Cargo

From the repository root:

```sh
cargo xrun
```

Run the headless agent path with serial log capture:

```sh
cargo xrun -- --agent
```

## Rootfs and disk image

Build or refresh `disk.img` and the guest root filesystem:

```sh
cargo xrootfs
```

Force rebuilding the disk image from scratch:

```sh
cargo xrootfs-override
```

Mount `sysroot/` from `disk.img` when needed:

```sh
cargo xsysroot-mount
```

## Tests

Run kernel unit tests in QEMU:

```sh
cargo xtest
```

Run integration tests:

```sh
cargo xintegration-test
```

## Notes

- `cargo xrun` is the main local workflow entrypoint.
- `nix run` uses the same xtask-based flow.
- If `/dev/kvm` exists, QEMU will use KVM automatically.
