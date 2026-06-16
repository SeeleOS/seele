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

## Agent MCP workflow

For Codex-driven work, prefer the Seele MCP server over manual QEMU control when it is registered in your MCP configuration. The dev shell provides a `seele-mcp` command:

```sh
nix develop -c seele-mcp
```

The MCP server exposes tools for the full agent loop:

- `run_xtest`: run kernel unit tests through the existing cargo alias.
- `agent_start`: build and launch the agent VM through `xtask mcp-run`.
- `agent_status`: report runner/QEMU PIDs, QMP connectivity, and serial log location.
- `agent_serial_tail`: read recent serial output.
- `agent_screenshot`: capture the display through QMP `screendump`.
- `agent_send_key`, `agent_type_text`, `agent_mouse_move`, `agent_mouse_click`: drive guest input through QMP.
- `agent_stop` and `agent_cleanup`: stop MCP-managed runner/QEMU processes and clean QMP socket state.

Manual `cargo xrun` and `cargo xrun -- --agent` remain useful for local foreground runs and fallback verification when MCP is unavailable.

## Rootfs and disk image

Build or refresh `disk.img` and the guest root filesystem:

```sh
cargo xbuild-rootfs
```

The rootfs is Alpine Linux based, uses OpenRC for `/sbin/init`, and installs a small base development package set through `apk`.

Force rebuilding the disk image from scratch:

```sh
cargo xbuild-rootfs -- --override
```

Mount `sysroot/` from `disk.img` through the MCP `ensure_sysroot_mounted` tool when needed.

## Tests

Run kernel and integration coverage in QEMU:

```sh
cargo xtest
```

## Notes

- `cargo xrun` is the main local workflow entrypoint.
- `nix run` uses the same xtask-based flow.
- If `/dev/kvm` exists, QEMU will use KVM automatically.
