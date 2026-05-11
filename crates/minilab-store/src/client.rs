use reqwest::{header, Client, Url};
use thiserror::Error;

use minilab_core::{SimBranch, SimMode};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("supabase error {status}: {body}")]
    Supabase { status: u16, body: String },
    #[error("env var missing: {0}")]
    Env(String),
    #[error("invalid env value for {var}: {value}")]
    InvalidEnv { var: String, value: String },
    #[error("provider {provider} error {status}: {body}")]
    Provider {
        provider: &'static str,
        status: u16,
        body: String,
    },
    #[error("contract violation: {0}")]
    Contract(String),
    #[error("send refused: sim_mode={mode:?} blocks real outbound")]
    SendBlocked { mode: SimMode },
}

/// Thin wrapper around a reqwest Client pointed at one Supabase project.
#[derive(Clone)]
pub struct StoreClient {
    pub http: Client,
    pub base_url: String,
    pub sim_mode: SimMode,
    pub sim_branch: SimBranch,
}

impl StoreClient {
    /// Build from explicit values (tests, integration).
    pub fn new(supabase_url: impl Into<String>, admin_key: impl Into<String>) -> Self {
        Self::with_mode(supabase_url, admin_key, SimMode::default())
    }

    pub fn with_mode(
        supabase_url: impl Into<String>,
        admin_key: impl Into<String>,
        sim_mode: SimMode,
    ) -> Self {
        let base_url = supabase_url.into();
        let key = admin_key.into();
        let sim_branch = SimBranch::for_mode(sim_mode);
        let mut headers = header::HeaderMap::new();
        headers.insert("apikey", header::HeaderValue::from_str(&key).unwrap());
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {key}")).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let mut builder = Client::builder().default_headers(headers);
        if is_loopback_url(&base_url) {
            builder = builder.no_proxy();
        }

        let http = builder.build().expect("reqwest client");

        Self {
            http,
            base_url,
            sim_mode,
            sim_branch,
        }
    }

    /// Build from `SUPABASE_URL` and an elevated server-side Supabase API key.
    ///
    /// Key lookup order:
    /// 1. `SUPABASE_SECRET_KEY` (preferred modern `sb_secret_...` key)
    /// 2. `SUPABASE_SERVICE_KEY` (legacy JWT `service_role` fallback)
    ///
    /// `MINILAB_SIM_MODE` (optional) selects the execution mode; values:
    /// `production` (default), `replay`, `simulation`, `counterfactual`.
    pub fn from_env() -> Result<Self, StoreError> {
        let url =
            std::env::var("SUPABASE_URL").map_err(|_| StoreError::Env("SUPABASE_URL".into()))?;
        let key = load_supabase_admin_key()?;
        let sim_mode = match std::env::var("MINILAB_SIM_MODE").ok().as_deref() {
            None | Some("") | Some("production") => SimMode::Production,
            Some("replay") => SimMode::Replay,
            Some("simulation") => SimMode::Simulation,
            Some("counterfactual") => SimMode::Counterfactual,
            Some(other) => {
                return Err(StoreError::InvalidEnv {
                    var: "MINILAB_SIM_MODE".into(),
                    value: other.into(),
                })
            }
        };
        let mut client = Self::with_mode(url, key, sim_mode);
        client.sim_branch = load_sim_branch(sim_mode);
        Ok(client)
    }

    pub fn rest(&self, table: &str) -> String {
        format!("{}/rest/v1/{}", self.base_url, table)
    }

    pub fn rpc(&self, name: &str) -> String {
        format!("{}/rest/v1/rpc/{}", self.base_url, name)
    }

    pub fn fork_counterfactual(&self) -> Self {
        let mut fork = self.clone();
        fork.sim_mode = SimMode::Counterfactual;
        fork.sim_branch =
            SimBranch::fork(SimMode::Counterfactual, self.sim_branch.branch_id.clone());
        fork
    }
}

fn load_sim_branch(sim_mode: SimMode) -> SimBranch {
    let branch_id = std::env::var("MINILAB_SIM_BRANCH_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let parent_branch_id = std::env::var("MINILAB_SIM_PARENT_BRANCH_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());

    match branch_id {
        Some(branch_id) => SimBranch::from_parts(sim_mode, branch_id, parent_branch_id),
        None => SimBranch::for_mode(sim_mode),
    }
}

fn is_loopback_url(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };

    let Some(host) = url.host_str() else {
        return false;
    };

    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn load_supabase_admin_key() -> Result<String, StoreError> {
    std::env::var("SUPABASE_SECRET_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("SUPABASE_SERVICE_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| StoreError::Env("SUPABASE_SECRET_KEY or SUPABASE_SERVICE_KEY".into()))
}
