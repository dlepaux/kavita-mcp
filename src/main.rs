//! kavita-mcp: serve a household's Kavita as two MCP tools on one listener. Every secret is required; a missing one stops the process before it binds.

use std::sync::Arc;
use std::time::Duration;

use kavita_mcp::config::Config;
use kavita_mcp::handles::Handles;
use kavita_mcp::http::{build_router, App};
use kavita_mcp::kavita::Kavita;
use kavita_mcp::metrics::Metrics;
use tracing_subscriber::EnvFilter;

/// A cold Kavita (one woken on demand by a proxy) plus an SMTP send of up to 25 MB.
const KAVITA_TIMEOUT: Duration = Duration::from_secs(190);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A deploy pre-flight can run a new image once with no env: this answers before configuration,
    // so it proves only that the binary loads.
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("kavita-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_vars(&std::env::vars().collect())?;
    let app = Arc::new(App {
        kavita: Kavita::new(&config.kavita_url, KAVITA_TIMEOUT),
        metrics: Metrics::new(),
        handles: Handles::default(),
        send_size_limit: config.send_size_limit,
        identities: config.identities,
    });

    let listener = tokio::net::TcpListener::bind(&config.addr).await?;
    tracing::info!(
        addr = %config.addr,
        people = app.identities.len(),
        "kavita-mcp serving /mcp, /metrics, /healthz"
    );
    axum::serve(listener, build_router(app))
        .with_graceful_shutdown(async {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler installs");
            tokio::select! {
                _ = term.recv() => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        })
        .await?;
    Ok(())
}
