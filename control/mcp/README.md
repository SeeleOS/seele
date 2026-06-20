# Seele Control MCP

This crate is the MCP-first entry point for the non-kernel control plane.

Long-running work returns a structured `JobStatus`. Raw process output is written
as artifacts and is not used as the agent observation surface.

