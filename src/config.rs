use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub upstream_base_url: Option<String>,
    pub redis_url: String,
    pub batch_window_secs: u64,
    pub batch_poll_interval_secs: u64,
    pub server_host: String,
    pub server_port: u16,
    pub tcp_keepalive_secs: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenv::dotenv().ok();

        Ok(Self {
            upstream_base_url: env::var("UPSTREAM_BASE_URL").ok(),
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            batch_window_secs: env::var("BATCH_WINDOW_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()?,
            batch_poll_interval_secs: env::var("BATCH_POLL_INTERVAL_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()?,
            server_host: env::var("SERVER_HOST")
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            server_port: env::var("SERVER_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()?,
            tcp_keepalive_secs: env::var("TCP_KEEPALIVE_SECS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()?,
        })
    }
}
