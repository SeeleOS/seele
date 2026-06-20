mod server;

use anyhow::Result;
use rmcp::{ServiceExt, transport};

#[tokio::main]
async fn main() -> Result<()> {
    let server = server::ControlMcp::new()?;
    let service = server.serve(transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
