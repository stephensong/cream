use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use clap::Parser;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;
use tracing_subscriber::EnvFilter;

mod contracts;
mod store;
mod subscriptions;
mod ws;

use store::Store;
use subscriptions::SubscriptionManager;

#[derive(Parser)]
#[command(name = "cream-node", about = "Postgres-backed Freenet-compatible node for CREAM")]
struct Cli {
    /// WebSocket listen ports (comma-separated). All ports share the same Postgres backend.
    #[arg(long, default_value = "3001", value_delimiter = ',')]
    port: Vec<u16>,

    /// Postgres connection string (libpq key=value or URL format)
    #[arg(long, default_value = "host=/var/run/postgresql dbname=cream_dev")]
    database_url: String,
}

pub struct AppState {
    pub store: Store,
    pub subscriptions: SubscriptionManager,
}

#[derive(serde::Deserialize)]
struct WsQuery {
    #[serde(rename = "encodingProtocol")]
    encoding_protocol: Option<String>,
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    if query.encoding_protocol.as_deref() != Some("native") {
        tracing::warn!(
            "Connection with encodingProtocol={:?} (expected 'native')",
            query.encoding_protocol
        );
    }
    ws.on_upgrade(move |socket| ws::handle_connection(socket, state))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Parse connection string — supports both libpq key=value and URL format
    let pg_config: tokio_postgres::Config = cli.database_url.parse()?;
    let mgr_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let mgr = Manager::from_config(pg_config, NoTls, mgr_config);
    let pool = Pool::builder(mgr).max_size(16).build()?;

    let store = Store::new(pool);
    store.run_migrations().await?;
    tracing::info!("Migrations applied");

    let state = Arc::new(AppState {
        store,
        subscriptions: SubscriptionManager::new(),
    });

    let app = Router::new()
        .route("/v1/contract/command", get(ws_upgrade))
        .with_state(state);

    let ports = cli.port;

    // Spawn a listener for each port, all sharing the same AppState
    let mut handles = Vec::new();
    for (i, &port) in ports.iter().enumerate() {
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
        tracing::info!("cream-node listening on port {port}");
        let app = app.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("Server on port {port} exited: {e}");
            }
        });
        if i == 0 {
            handles.push(handle);
        }
    }

    // Wait for the first server to exit (keeps process alive)
    if let Some(handle) = handles.into_iter().next() {
        handle.await?;
    }

    Ok(())
}
