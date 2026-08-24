// SPDX-License-Identifier: AGPL-3.0-only

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use struxio_api::AppState;
use struxio_mcp::{McpServer, ServiceAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let api_key = std::env::var("STRUXIO_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        anyhow::bail!("STRUXIO_API_KEY must be set");
    }

    let config = struxio_common::config::Config::from_env()
        .map_err(|error| anyhow::anyhow!("failed to load configuration: {error}"))?;

    let db_pool = struxio_db::create_pool(&config.database_url).await?;
    tracing::info!("Running database migrations...");
    sqlx::migrate!("../../migrations")
        .run(&db_pool)
        .await
        .map_err(|error| anyhow::anyhow!("failed to run database migrations: {error}"))?;

    let app_state = AppState::new(config, db_pool).await?;
    let principal = app_state.local_principal.clone();
    if principal.workspace_id().as_uuid().is_nil() {
        anyhow::bail!("refusing to start MCP with a nil workspace principal");
    }

    let adapter = ServiceAdapter::new(
        app_state.template_service,
        app_state.extraction_service,
        app_state.batch_service,
        app_state.model_service,
    );
    tracing::info!(
        workspace_id = %principal.workspace_id(),
        "Starting Struxio MCP stdio server"
    );

    let server = McpServer::new(adapter, principal);
    server.serve_stdio().await?;
    Ok(())
}
