mod batch_worker;
mod config;
mod handlers;
mod models;
mod openai_client;
mod state;

use axum::{
    routing::{get, post},
    Router,
};
use batch_worker::BatchWorker;
use config::Config;
use handlers::{AppState, create_chat_completion, health_check};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use socket2::TcpKeepalive;
use state::StateManager;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    info!("Starting OpenAI Batch Proxy");

    // Load configuration
    let config = Arc::new(Config::from_env()?);
    info!("Configuration loaded");
    info!("Batch window: {}s", config.batch_window_secs);
    info!("Batch poll interval: {}s", config.batch_poll_interval_secs);
    info!("TCP keepalive: {}s", config.tcp_keepalive_secs);

    // Initialize state manager
    let state_manager = StateManager::new(&config.redis_url).await?;
    info!("Connected to Redis at {}", config.redis_url);

    // Create app state
    let app_state = Arc::new(AppState { state_manager: state_manager.clone() });

    // Create batch worker
    let batch_worker = Arc::new(BatchWorker::new(Arc::clone(&config), state_manager));

    // Start batch dispatcher
    let dispatcher_worker = Arc::clone(&batch_worker);
    tokio::spawn(async move {
        dispatcher_worker.start_dispatcher().await;
    });
    info!("Batch dispatcher started");

    // Start existing batch poller
    let poller_worker = Arc::clone(&batch_worker);
    tokio::spawn(async move {
        poller_worker.start_poller().await;
    });
    info!("Batch poller started");

    // Build router
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(create_chat_completion))
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
        .with_state(app_state);

    // Bind to address
    let addr: SocketAddr = format!("{}:{}", config.server_host, config.server_port).parse()?;
    info!("Binding to {}", addr);

    // Create TCP listener with custom socket options
    let std_listener = std::net::TcpListener::bind(addr)?;
    std_listener.set_nonblocking(true)?;

    let listener = TcpListener::from_std(std_listener)?;

    info!("Server listening on {}", addr);
    info!("Ready to accept requests");

    // Accept connections with TCP keepalive
    loop {
        let (socket, remote_addr) = listener.accept().await?;

        // Configure TCP keepalive
        let socket_ref = socket2::SockRef::from(&socket);
        let keepalive = TcpKeepalive::new()
            .with_time(Duration::from_secs(config.tcp_keepalive_secs))
            .with_interval(Duration::from_secs(30));

        socket_ref.set_tcp_keepalive(&keepalive)?;

        // Disable Nagle's algorithm for lower latency
        socket_ref.set_nodelay(true)?;

        let tower_service = app.clone();

        tokio::spawn(async move {
            let socket = TokioIo::new(socket);

            // Convert tower service to hyper service
            let hyper_service = TowerToHyperService::new(tower_service);

            // Serve connection with very long timeouts
            let conn = http1::Builder::new()
                .keep_alive(true)
                .serve_connection(socket, hyper_service);

            if let Err(err) = conn.await {
                tracing::error!("Error serving connection from {}: {}", remote_addr, err);
            }
        });
    }
}
