use crate::{qmp, session::AgentSession};
use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::to_string_pretty;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SeeleMcp {
    session: Arc<AgentSession>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl SeeleMcp {
    pub fn new() -> Result<Self> {
        Ok(Self {
            session: Arc::new(AgentSession::from_env()?),
            tool_router: Self::tool_router(),
        })
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SerialTailRequest {
    pub lines: Option<usize>,
    pub bytes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SendKeyRequest {
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeTextRequest {
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MouseMoveRequest {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MouseClickRequest {
    pub button: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DebugStartRequest {
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DebugCommandRequest {
    pub command: String,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StartRequest {
    pub enable_profiling: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunTestsRequest {
    pub test: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BuildRootfsRequest {
    pub timeout_ms: Option<u64>,
    pub override_rootfs: Option<bool>,
    pub rebuild_aur: Option<bool>,
    pub rebuild_aur_packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CommandStatusRequest {
    pub id: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CommandWaitRequest {
    pub id: u64,
    pub timeout_ms: Option<u64>,
}

#[tool_router]
impl SeeleMcp {
    #[tool(description = "Start the Seele OS VM session through xtask mcp-run")]
    async fn start(&self, Parameters(request): Parameters<StartRequest>) -> CallToolResult {
        json_result(
            self.session
                .start(request.enable_profiling.unwrap_or(false))
                .await,
        )
        .await
    }

    #[tool(description = "Stop the Seele OS VM session managed by this MCP server")]
    async fn stop(&self) -> CallToolResult {
        json_result(self.session.stop().await).await
    }

    #[tool(description = "Report Seele OS VM process, serial log, and QMP socket status")]
    async fn status(&self) -> CallToolResult {
        json_result(self.session.status().await).await
    }

    #[tool(description = "Return the tail of the Seele OS serial log")]
    async fn serial_tail(
        &self,
        Parameters(request): Parameters<SerialTailRequest>,
    ) -> CallToolResult {
        text_result(self.session.serial_tail(request.lines, request.bytes).await)
    }

    #[tool(description = "Capture a Seele OS VM screenshot through QMP screendump")]
    async fn screenshot(&self) -> CallToolResult {
        match qmp::screendump_png(self.session.qmp_socket()).await {
            Ok(png) => {
                CallToolResult::success(vec![Content::image(STANDARD.encode(png), "image/png")])
            }
            Err(err) => tool_error(err),
        }
    }

    #[tool(description = "Send a QMP keyboard key or key combination to the VM")]
    async fn send_key(&self, Parameters(request): Parameters<SendKeyRequest>) -> CallToolResult {
        unit_result(qmp::send_key(self.session.qmp_socket(), &request.keys).await)
    }

    #[tool(description = "Type ASCII text into the VM through QMP keyboard events")]
    async fn type_text(&self, Parameters(request): Parameters<TypeTextRequest>) -> CallToolResult {
        unit_result(qmp::type_text(self.session.qmp_socket(), &request.text).await)
    }

    #[tool(description = "Move the VM absolute pointer through QMP")]
    async fn mouse_move(
        &self,
        Parameters(request): Parameters<MouseMoveRequest>,
    ) -> CallToolResult {
        unit_result(qmp::mouse_move(self.session.qmp_socket(), request.x, request.y).await)
    }

    #[tool(description = "Click a VM mouse button through QMP")]
    async fn mouse_click(
        &self,
        Parameters(request): Parameters<MouseClickRequest>,
    ) -> CallToolResult {
        unit_result(qmp::mouse_click(self.session.qmp_socket(), &request.button).await)
    }

    #[tool(description = "Clean up the MCP-managed Seele OS VM session")]
    async fn cleanup(&self) -> CallToolResult {
        json_result(self.session.cleanup().await).await
    }

    #[tool(description = "Start a Seele OS VM paused at QEMU's GDB stub and attach gdb")]
    async fn debug_start(
        &self,
        Parameters(request): Parameters<DebugStartRequest>,
    ) -> CallToolResult {
        json_result(self.session.debug_start(request.port).await).await
    }

    #[tool(description = "Run a command in the active Seele OS gdb session")]
    async fn debug_command(
        &self,
        Parameters(request): Parameters<DebugCommandRequest>,
    ) -> CallToolResult {
        json_result(
            self.session
                .debug_command(&request.command, request.timeout_ms)
                .await,
        )
        .await
    }

    #[tool(description = "Report the active Seele OS gdb debugging session status")]
    async fn debug_status(&self) -> CallToolResult {
        json_result(self.session.debug_status().await).await
    }

    #[tool(description = "Stop the active Seele OS gdb session and VM")]
    async fn debug_stop(&self) -> CallToolResult {
        json_result(self.session.debug_stop().await).await
    }

    #[tool(
        description = "Start cargo xtest for Seele OS as a background MCP job. By default this runs kernel unit tests plus LTP. Pass \"full\" to run every integration test, or pass a test name filter."
    )]
    async fn run_tests(&self, Parameters(request): Parameters<RunTestsRequest>) -> CallToolResult {
        json_result(self.session.start_xtest(request.test.as_deref()).await).await
    }

    #[tool(description = "Start cargo xbuild-rootfs for Seele OS as a background MCP job")]
    async fn build_rootfs(
        &self,
        Parameters(request): Parameters<BuildRootfsRequest>,
    ) -> CallToolResult {
        json_result(
            self.session
                .start_build_rootfs(
                    request.timeout_ms,
                    request.override_rootfs.unwrap_or(false),
                    request.rebuild_aur.unwrap_or(false),
                    request.rebuild_aur_packages.as_deref().unwrap_or(&[]),
                )
                .await,
        )
        .await
    }

    #[tool(description = "Report a background MCP xtask job status and output when finished")]
    async fn command_status(
        &self,
        Parameters(request): Parameters<CommandStatusRequest>,
    ) -> CallToolResult {
        json_result(self.session.command_status(request.id).await).await
    }

    #[tool(description = "Wait for a background MCP xtask job to finish and return final status")]
    async fn command_wait(
        &self,
        Parameters(request): Parameters<CommandWaitRequest>,
    ) -> CallToolResult {
        json_result(
            self.session
                .command_wait(request.id, request.timeout_ms)
                .await,
        )
        .await
    }

    #[tool(description = "Ensure rootfs_mnt/ is mounted from rootfs.img")]
    async fn ensure_rootfs_mounted(&self) -> CallToolResult {
        json_result(self.session.ensure_rootfs_mounted().await).await
    }

    #[tool(description = "Unmount rootfs_mnt/ if it is currently mounted")]
    async fn unmount_rootfs(&self) -> CallToolResult {
        json_result(self.session.unmount_rootfs().await).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SeeleMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Seele OS VM workflow server")
    }
}

async fn json_result<T: serde::Serialize>(result: Result<T>) -> CallToolResult {
    match result.and_then(|value| to_string_pretty(&value).map_err(Into::into)) {
        Ok(text) => CallToolResult::success(vec![Content::text(text)]),
        Err(err) => tool_error(err),
    }
}

fn text_result(result: Result<String>) -> CallToolResult {
    match result {
        Ok(text) => CallToolResult::success(vec![Content::text(text)]),
        Err(err) => tool_error(err),
    }
}

fn unit_result(result: Result<()>) -> CallToolResult {
    match result {
        Ok(()) => CallToolResult::success(vec![Content::text("ok")]),
        Err(err) => tool_error(err),
    }
}

fn tool_error(err: anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
}
