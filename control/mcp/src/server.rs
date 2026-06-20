use anyhow::Result;
use control_core::{
    plane::ControlPlane, qemu::VmConfig, rootfs::BuildRootfsConfig, tests::RunTestsConfig,
};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::to_string_pretty;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct ControlMcp {
    plane: ControlPlane,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl ControlMcp {
    pub fn new() -> Result<Self> {
        let repo = std::env::current_dir()?;
        Ok(Self {
            plane: ControlPlane::new(repo),
            tool_router: Self::tool_router(),
        })
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunTestsRequest {
    pub selector: Option<String>,
    pub test: Option<String>,
    pub ltp_suite: Option<String>,
    pub ltp_pattern: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BuildRootfsRequest {
    pub override_rootfs: Option<bool>,
    pub rebuild_aur: Option<bool>,
    pub rebuild_aur_packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StartVmRequest {
    pub qmp_socket: Option<PathBuf>,
    pub serial_log: Option<PathBuf>,
    pub rootfs_image: Option<PathBuf>,
    pub ltp_device_image: Option<PathBuf>,
    pub iso_image: Option<PathBuf>,
    pub enable_profiling: Option<bool>,
    pub display: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct JobRequest {
    pub id: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct JobWaitRequest {
    pub id: u64,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SerialTailRequest {
    pub lines: Option<usize>,
    pub bytes: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitSerialRequest {
    pub pattern: String,
    pub timeout_ms: Option<u64>,
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

#[tool_router]
impl ControlMcp {
    #[tool(description = "Start a structured Seele OS test job")]
    async fn run_tests(&self, Parameters(request): Parameters<RunTestsRequest>) -> CallToolResult {
        json_result(Ok(self.plane.start_tests(RunTestsConfig {
            selector: request.selector.or(request.test),
            ltp_suite: request.ltp_suite,
            ltp_pattern: request.ltp_pattern,
        })))
    }

    #[tool(description = "Start a structured Arch rootfs build job")]
    async fn build_rootfs(
        &self,
        Parameters(request): Parameters<BuildRootfsRequest>,
    ) -> CallToolResult {
        json_result(Ok(self.plane.start_build_rootfs(BuildRootfsConfig {
            override_rootfs: request.override_rootfs.unwrap_or(false),
            rebuild_aur: request.rebuild_aur.unwrap_or(false),
            rebuild_aur_packages: request.rebuild_aur_packages.unwrap_or_default(),
        })))
    }

    #[tool(description = "Start a managed Seele OS QEMU VM job")]
    async fn start_vm(&self, Parameters(request): Parameters<StartVmRequest>) -> CallToolResult {
        let repo = std::env::current_dir().expect("failed to resolve current directory");
        let mut config = VmConfig::for_repo(&repo);
        if let Some(path) = request.qmp_socket {
            config.qmp_socket = path;
        }
        if let Some(path) = request.serial_log {
            config.serial_log = path;
        }
        if let Some(path) = request.rootfs_image {
            config.rootfs_image = path;
        }
        if let Some(path) = request.ltp_device_image {
            config.ltp_device_image = path;
        }
        if let Some(path) = request.iso_image {
            config.iso_image = Some(path);
        }
        config.enable_profiling = request.enable_profiling.unwrap_or(false);
        config.display = request.display.unwrap_or(false);
        json_result(Ok(self.plane.start_vm(config)))
    }

    #[tool(description = "Stop the managed Seele OS QEMU VM")]
    async fn stop_vm(&self) -> CallToolResult {
        json_result(Ok(self.plane.stop_vm()))
    }

    #[tool(description = "Report managed VM status")]
    async fn vm_status(&self) -> CallToolResult {
        json_result(Ok(self.plane.vm_status()))
    }

    #[tool(description = "Return the serial log tail")]
    async fn serial_tail(
        &self,
        Parameters(request): Parameters<SerialTailRequest>,
    ) -> CallToolResult {
        text_result(self.plane.serial_tail(request.lines, request.bytes))
    }

    #[tool(description = "Capture a VM screenshot through QMP screendump")]
    async fn screenshot(&self) -> CallToolResult {
        match self
            .plane
            .screenshot()
            .and_then(|path| fs::read(path).map_err(Into::into))
        {
            Ok(bytes) => CallToolResult::success(vec![Content::image(
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
                "image/x-portable-pixmap",
            )]),
            Err(err) => tool_error(err),
        }
    }

    #[tool(description = "Start a job that waits for a serial log pattern")]
    async fn wait_serial(
        &self,
        Parameters(request): Parameters<WaitSerialRequest>,
    ) -> CallToolResult {
        json_result(Ok(self
            .plane
            .wait_serial(request.pattern, request.timeout_ms)))
    }

    #[tool(description = "Return structured job status")]
    async fn job_status(&self, Parameters(request): Parameters<JobRequest>) -> CallToolResult {
        json_result(self.plane.jobs().status(request.id))
    }

    #[tool(description = "Wait for a structured job to reach a terminal state")]
    async fn job_wait(&self, Parameters(request): Parameters<JobWaitRequest>) -> CallToolResult {
        json_result(self.plane.jobs().wait(request.id, request.timeout_ms))
    }

    #[tool(description = "Cancel a structured job")]
    async fn job_cancel(&self, Parameters(request): Parameters<JobRequest>) -> CallToolResult {
        json_result(self.plane.jobs().cancel(request.id))
    }

    #[tool(description = "Ensure target/rootfs_mnt is mounted from target/rootfs.img")]
    async fn ensure_rootfs_mounted(&self) -> CallToolResult {
        json_result(Ok(self.plane.ensure_rootfs_mounted()))
    }

    #[tool(description = "Unmount target/rootfs_mnt if mounted")]
    async fn unmount_rootfs(&self) -> CallToolResult {
        json_result(Ok(self.plane.unmount_rootfs()))
    }

    #[tool(description = "Compatibility alias for start_vm")]
    async fn start(&self, Parameters(request): Parameters<StartVmRequest>) -> CallToolResult {
        self.start_vm(Parameters(request)).await
    }

    #[tool(description = "Compatibility alias for stop_vm")]
    async fn stop(&self) -> CallToolResult {
        self.stop_vm().await
    }

    #[tool(description = "Compatibility alias for vm_status")]
    async fn status(&self) -> CallToolResult {
        self.vm_status().await
    }

    #[tool(description = "Compatibility alias for job_status")]
    async fn command_status(&self, Parameters(request): Parameters<JobRequest>) -> CallToolResult {
        self.job_status(Parameters(request)).await
    }

    #[tool(description = "Compatibility alias for job_wait")]
    async fn command_wait(
        &self,
        Parameters(request): Parameters<JobWaitRequest>,
    ) -> CallToolResult {
        self.job_wait(Parameters(request)).await
    }

    #[tool(description = "Compatibility alias for job_cancel")]
    async fn command_cancel(&self, Parameters(request): Parameters<JobRequest>) -> CallToolResult {
        self.job_cancel(Parameters(request)).await
    }

    #[tool(description = "Send a QMP keyboard key or key combination to the VM")]
    async fn send_key(&self, Parameters(request): Parameters<SendKeyRequest>) -> CallToolResult {
        unit_result(self.plane.send_key(&request.keys))
    }

    #[tool(description = "Type ASCII text into the VM through QMP keyboard events")]
    async fn type_text(&self, Parameters(request): Parameters<TypeTextRequest>) -> CallToolResult {
        unit_result(self.plane.type_text(&request.text))
    }

    #[tool(description = "Move the VM absolute pointer through QMP")]
    async fn mouse_move(
        &self,
        Parameters(request): Parameters<MouseMoveRequest>,
    ) -> CallToolResult {
        unit_result(self.plane.mouse_move(request.x, request.y))
    }

    #[tool(description = "Click a VM mouse button through QMP")]
    async fn mouse_click(
        &self,
        Parameters(request): Parameters<MouseClickRequest>,
    ) -> CallToolResult {
        unit_result(self.plane.mouse_click(&request.button))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ControlMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Seele OS structured control-plane server")
    }
}

fn json_result<T: serde::Serialize>(result: Result<T>) -> CallToolResult {
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
        Ok(()) => CallToolResult::success(vec![Content::text("{}")]),
        Err(err) => tool_error(err),
    }
}

fn tool_error(err: anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![Content::text(format!("{err:#}"))])
}
