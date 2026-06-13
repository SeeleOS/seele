mod qmp;
mod server;
mod session;

use anyhow::Result;
use rmcp::{ServiceExt, transport};

#[tokio::main]
async fn main() -> Result<()> {
    let server = server::SeeleMcp::new()?;
    let service = server.serve(transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
