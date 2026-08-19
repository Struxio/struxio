use std::env;
use struxio_api::AppState;
use struxio_common::config::Config;
use struxio_mcp::{McpServer, ServiceAdapter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()
        .map_err(|error| anyhow::anyhow!("failed to load MCP configuration: {error}"))?;
    require_stdio_auth_configuration()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let db_pool = struxio_db::create_pool(&config.database_url).await?;
    sqlx::migrate!("../../migrations").run(&db_pool).await?;
    let state = AppState::new(config, db_pool).await?;

    // stdio is authenticated by the process boundary. The principal is still
    // loaded from the workspace membership table and is passed explicitly to
    // the adapter; no nil or implicit workspace can reach a service call.
    tracing::info!(
        workspace_id = %state.local_principal.workspace_id(),
        principal_id = %state.local_principal.principal_id(),
        "starting authenticated Struxio MCP stdio server"
    );
    let adapter = ServiceAdapter::new(
        state.template_service,
        state.extraction_service,
        state.batch_service,
        state.model_service,
        state.local_principal,
    );
    McpServer::new(adapter).serve_stdio().await?;
    Ok(())
}

fn require_stdio_auth_configuration() -> anyhow::Result<()> {
    let configured = env::var("STRUXIO_MCP_API_KEY")
        .or_else(|_| env::var("STRUXIO_API_KEY"))
        .unwrap_or_default();
    if configured.trim().is_empty() {
        anyhow::bail!(
            "MCP stdio requires STRUXIO_MCP_API_KEY or STRUXIO_API_KEY; \
             stdio process authentication must be explicit"
        );
    }
    Ok(())
}
