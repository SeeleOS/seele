# Seele MCP

`seele-mcp` is the repository-local MCP server for Codex-driven Seele OS work. It is intentionally scoped to this repository's VM workflow, not a general QEMU controller.

## Tools

- `start`: run `cargo run -p xtask -- mcp-run`, build the kernel, create the Limine ISO, launch QEMU, and capture machine-readable metadata.
- `status`: report runner/QEMU PIDs, QMP socket path/connectivity, serial log path, and last exit status.
- `serial_tail`: return the latest serial log lines or bytes.
- `screenshot`: use QMP `screendump`, convert PPM to PNG, and return image content.
- `send_key`, `type_text`: send keyboard input through QMP. Text input is ASCII-only.
- `mouse_move`, `mouse_click`: send absolute pointer and button events through QMP.
- `stop`, `cleanup`: terminate the MCP-managed VM and remove QMP socket state.
- `debug_start`: start the VM paused at QEMU's GDB stub and attach `gdb` to the kernel ELF.
- `debug_command`: run a command in the active GDB session and return output up to the next prompt.
- `debug_status`, `debug_stop`: inspect or stop the active GDB-backed VM session.
- `run_tests`, `build_rootfs`: run the existing cargo aliases and return truncated logs.
- `ensure_rootfs_mounted`: mount `target/rootfs_mnt/` from `target/rootfs.img`, leaving an already mounted `target/rootfs_mnt/` unchanged.

## Local Run

```sh
nix develop -c seele-mcp
```

The Home Manager MCP configuration can point to the same command. In this environment, `/home/elysia/nix/home/mcp.nix` registers a `seele` server that enters this repo and runs `nix develop -c cargo run -p seele-mcp`.

## Smoke Checks

After changing MCP or VM launch behavior:

1. Start the VM with `start`.
2. Confirm `status` reports QMP connectable.
3. Check boot output with `serial_tail`.
4. Capture a non-empty screenshot with `screenshot`.
5. Stop the VM with `stop` or `cleanup`.
6. Confirm `status` is idle and no session metadata remains.

If the MCP server is unavailable, use `cargo xrun -- --agent` as the fallback path and clean up the reported runner/QEMU processes from the host.
