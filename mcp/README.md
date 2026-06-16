# Seele MCP

`seele-mcp` is the repository-local MCP server for Codex-driven Seele OS work. It is intentionally scoped to this repository's agent workflow, not a general QEMU controller.

## Tools

- `agent_start`: run `cargo run -p xtask -- mcp-run`, build the kernel, create the UEFI image, launch QEMU, and capture machine-readable metadata.
- `agent_status`: report runner/QEMU PIDs, QMP socket path/connectivity, serial log path, and last exit status.
- `agent_serial_tail`: return the latest serial log lines or bytes.
- `agent_screenshot`: use QMP `screendump`, convert PPM to PNG, and return image content.
- `agent_send_key`, `agent_type_text`: send keyboard input through QMP. Text input is ASCII-only.
- `agent_mouse_move`, `agent_mouse_click`: send absolute pointer and button events through QMP.
- `agent_stop`, `agent_cleanup`: terminate the MCP-managed VM and remove QMP socket state.
- `debug_start`: start the VM paused at QEMU's GDB stub and attach `gdb` to the kernel ELF.
- `debug_command`: run a command in the active GDB session and return output up to the next prompt.
- `debug_status`, `debug_stop`: inspect or stop the active GDB-backed VM session.
- `run_xtest`, `run_xintegration_test`, `run_xrootfs`: run the existing cargo aliases and return truncated logs.
- `ensure_sysroot_mounted`: run `cargo xsysroot-mount`, leaving an already mounted `sysroot/` unchanged.

## Local Run

```sh
nix develop -c seele-mcp
```

The Home Manager MCP configuration can point to the same command. In this environment, `/home/elysia/nix/home/mcp.nix` registers a `seele` server that enters this repo and runs `nix develop -c cargo run -p seele-mcp`.

## Smoke Checks

After changing MCP or VM launch behavior:

1. Start the VM with `agent_start`.
2. Confirm `agent_status` reports QMP connectable.
3. Check boot output with `agent_serial_tail`.
4. Capture a non-empty screenshot with `agent_screenshot`.
5. Stop the VM with `agent_stop` or `agent_cleanup`.
6. Confirm `agent_status` reports no managed runner/QEMU process.

If the MCP server is unavailable, use `cargo xrun -- --agent` as the fallback path and clean up the reported runner/QEMU processes from the host.
