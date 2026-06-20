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

The MCP server exposes tools for the structured control-plane loop:

- `run_tests`: start a structured test job and return typed reports/artifacts.
- `start_vm` / `start`: launch the VM through `control-core`.
- `vm_status` / `status`: report QEMU PID, QMP connectivity, and serial log location.
- `serial_tail`: read recent serial output.
- `job_status`, `job_wait`, `job_cancel`: inspect and control structured jobs.
- `stop_vm` / `stop`: stop MCP-managed QEMU state.

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

Run kernel unit and LTP coverage:

```sh
cargo xtest
```

## Notes

- `cargo xrun` is the main local workflow entrypoint.
- `nix run` uses the same `control-cli` flow.
- If `/dev/kvm` exists, QEMU will use KVM automatically.
