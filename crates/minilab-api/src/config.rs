use std::{net::SocketAddr, str::FromStr, time::Duration};

use minilab_store::StoreError;

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind_addr: SocketAddr,
    pub public_base_url: Option<String>,
    pub request_timeout: Duration,
    pub twilio_max_body_bytes: usize,
    pub sendgrid_max_body_bytes: usize,
    pub twilio_auth_token: Option<String>,
    pub sendgrid_parse_public_key: Option<String>,
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, StoreError> {
        Ok(Self {
            bind_addr: parse_socket_addr_env("MINILAB_HTTP_BIND_ADDR", "0.0.0.0:3000")?,
            public_base_url: std::env::var("MINILAB_PUBLIC_BASE_URL")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            request_timeout: Duration::from_secs(parse_u64_env("MINILAB_HTTP_TIMEOUT_SECS", 30)?),
            twilio_max_body_bytes: parse_usize_env("MINILAB_HTTP_MAX_TWILIO_BODY_BYTES", 262_144)?,
            sendgrid_max_body_bytes: parse_usize_env(
                "MINILAB_HTTP_MAX_SENDGRID_BODY_BYTES",
                10 * 1024 * 1024,
            )?,
            twilio_auth_token: std::env::var("TWILIO_AUTH_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            sendgrid_parse_public_key: std::env::var("SENDGRID_PARSE_PUBLIC_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        })
    }
}

fn parse_socket_addr_env(var: &str, default: &str) -> Result<SocketAddr, StoreError> {
    let value = std::env::var(var).unwrap_or_else(|_| default.to_string());
    SocketAddr::from_str(&value).map_err(|_| StoreError::InvalidEnv {
        var: var.into(),
        value,
    })
}

fn parse_u64_env(var: &str, default: u64) -> Result<u64, StoreError> {
    let value = std::env::var(var).unwrap_or_else(|_| default.to_string());
    value.parse::<u64>().map_err(|_| StoreError::InvalidEnv {
        var: var.into(),
        value,
    })
}

fn parse_usize_env(var: &str, default: usize) -> Result<usize, StoreError> {
    let value = std::env::var(var).unwrap_or_else(|_| default.to_string());
    value.parse::<usize>().map_err(|_| StoreError::InvalidEnv {
        var: var.into(),
        value,
    })
}
