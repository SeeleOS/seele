# Seele OS

Minimal build instructions.

## Prerequisites

- Linux host
- `git`
- `nix` with flakes enabled
- `qemu-system-x86_64`
- `sudo` access for mounting/unmounting `target/rootfs.img`

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

## Agent MCP workflow

For Codex-driven work, prefer the Seele MCP server over manual QEMU control when it is registered in your MCP configuration. The dev shell provides a `seele-mcp` command:

```sh
nix develop -c seele-mcp
```

The MCP server exposes tools for the full VM loop:

- `run_tests`: run kernel unit tests through the existing cargo alias.
- `start`: build and launch the VM through `xtask mcp-run`.
- `status`: report runner/QEMU PIDs, QMP connectivity, and serial log location.
- `serial_tail`: read recent serial output.
- `screenshot`: capture the display through QMP `screendump`.
- `send_key`, `type_text`, `mouse_move`, `mouse_click`: drive guest input through QMP.
- `stop` and `cleanup`: stop MCP-managed runner/QEMU processes and clean QMP socket state.

Manual `cargo xrun` and `cargo xrun -- --agent` remain useful for local foreground runs and fallback verification when MCP is unavailable.

## Rootfs and rootfs image

Build or refresh `target/rootfs.img` and the guest root filesystem:

```sh
cargo xbuild-rootfs
```

The rootfs is Arch Linux based and installs a small base development package set through `pacstrap`.

Force rebuilding the rootfs image from scratch:

```sh
cargo xbuild-rootfs -- --override
```

Mount `target/rootfs_mnt/` from `target/rootfs.img` through the MCP `ensure_rootfs_mounted` tool when needed.

## Tests

Run kernel and integration coverage in QEMU:

```sh
cargo xtest
```

## Notes

- `cargo xrun` is the main local workflow entrypoint.
- `nix run` uses the same xtask-based flow.
- If `/dev/kvm` exists, QEMU will use KVM automatically.
