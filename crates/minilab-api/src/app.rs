use std::{collections::BTreeMap, sync::Arc};

use axum::{
    body::Bytes,
    error_handling::HandleErrorLayer,
    extract::{MatchedPath, OriginalUri, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE, HOST},
        HeaderMap, HeaderName, Request, StatusCode, Uri,
    },
    response::IntoResponse,
    routing::{get, post},
    BoxError, Json, Router,
};
use futures_util::stream;
use minilab_store::{
    export_evidence_trail, lower_and_dispatch_execute,
    reply::{ingest_reply, IngestReplyInput, SendGridParsePayload, TwilioWhatsAppInboundPayload},
    webhook::{validate_sendgrid_signature, validate_twilio_signature},
    HostPairOutcome, InstallReconcileOutcome, OutboundSendOutcome, StoreClient, StoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower::{timeout::TimeoutLayer, ServiceBuilder};
use tower_http::{
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::{DefaultOnFailure, DefaultOnResponse, TraceLayer},
};
use tracing::{info_span, Level};
use uuid::Uuid;

use crate::{agent_runtime::AgentRuntimeService, config::ApiConfig, error::ApiError};

#[derive(Clone)]
pub struct AppState {
    pub store: StoreClient,
    pub config: Arc<ApiConfig>,
    pub agent_runtime: AgentRuntimeService,
}

#[derive(Debug, Serialize)]
struct HealthResponse<'a> {
    service: &'a str,
    status: &'a str,
    version: &'a str,
}

#[derive(Debug, Serialize)]
struct ReadinessResponse<'a> {
    service: &'a str,
    status: &'a str,
    sim_mode: String,
}

#[derive(Debug, Serialize)]
struct ObservabilityMetrics {
    service: String,
    sim_mode: String,
    sim_branch_id: String,
    sim_parent_branch_id: Option<String>,
    agent_sessions_total: usize,
    agent_sessions_pending: usize,
    evidence_close_span: &'static str,
}

#[derive(Debug, Serialize)]
struct OperatorDashboard {
    metrics: ObservabilityMetrics,
    recent_agent_sessions: Vec<crate::agent_runtime::AgentRuntimeSessionSnapshot>,
}

pub fn build_app(state: AppState) -> Router {
    let config = state.config.clone();
    let sensitive_headers = [
        AUTHORIZATION,
        HeaderName::from_static("x-twilio-signature"),
        HeaderName::from_static("x-twilio-email-event-webhook-signature"),
    ];
    let middleware = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(handle_middleware_error))
        .layer(TimeoutLayer::new(config.request_timeout))
        .layer(SetSensitiveRequestHeadersLayer::new(sensitive_headers))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<axum::body::Body>| {
                    let matched_path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str)
                        .unwrap_or(request.uri().path());
                    let request_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("missing");
                    info_span!(
                        "http.request",
                        method = %request.method(),
                        matched_path = %matched_path,
                        uri = %request.uri(),
                        request_id = %request_id
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO))
                .on_failure(DefaultOnFailure::new().level(Level::ERROR)),
        )
        .into_inner();

    let health_routes = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    let observability_routes = Router::new()
        .route("/metrics", get(observability_metrics))
        .route("/dashboard", get(operator_dashboard));

    let twilio_routes = Router::new()
        .route("/twilio/whatsapp", post(twilio_whatsapp_webhook))
        .layer(RequestBodyLimitLayer::new(config.twilio_max_body_bytes));

    let sendgrid_routes = Router::new()
        .route("/sendgrid/parse", post(sendgrid_parse_webhook))
        .layer(RequestBodyLimitLayer::new(config.sendgrid_max_body_bytes));

    let outbound_routes = Router::new().route("/send", post(outbound_send));
    let installation_routes = Router::new().route("/{id}/reconcile", post(install_reconcile));
    let evidence_routes = Router::new().route("/{correlation_id}/trail", get(evidence_trail));

    // Act-shaped surface (`POST /host-pairings`) rather than a REST-CRUD
    // mutation on `/hosts/:id/pair` — pairing is a constitutional act, not a
    // resource update. See bundle-into-grammar §4.
    let host_pairing_routes = Router::new().route("/", post(host_pair));
    let mcp_routes = crate::mcp_query::routes()
        .merge(crate::mcp_command::routes())
        .merge(crate::mcp_artifacts::routes());

    Router::new()
        .merge(health_routes)
        .nest("/operators", observability_routes)
        .nest("/webhooks", twilio_routes.merge(sendgrid_routes))
        .nest("/outbound", outbound_routes)
        .nest("/host-pairings", host_pairing_routes)
        .nest("/installations", installation_routes)
        .nest("/evidence", evidence_routes)
        .nest("/api/agent-runtime", crate::agent_runtime::routes())
        .nest("/mcp", mcp_routes)
        .with_state(state)
        .layer(middleware)
}

#[derive(Debug, Deserialize)]
struct HostPairRequest {
    host_id: Uuid,
    challenge: String,
    agent_pubkey: String,
    #[serde(default)]
    agent_signature: Option<String>,
    #[serde(default)]
    correlation_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum HostPairResponse {
    Paired {
        correlation_id: Uuid,
        pairing_session_id: Uuid,
    },
    Failed {
        correlation_id: Uuid,
        reason_code: String,
        reason_detail: String,
        phase: &'static str,
    },
}

/// Constitutional `host.pair` proving surface. See
/// `docs/integration/bundle-into-grammar.md` §4.
async fn host_pair(
    State(state): State<AppState>,
    Json(body): Json<HostPairRequest>,
) -> Result<Json<HostPairResponse>, ApiError> {
    let correlation_id = body.correlation_id.unwrap_or_else(Uuid::new_v4);
    let mut params = BTreeMap::new();
    params.insert("host_id".into(), json!(body.host_id));
    params.insert("challenge".into(), json!(body.challenge));
    params.insert("agent_pubkey".into(), json!(body.agent_pubkey));
    if let Some(agent_signature) = body.agent_signature {
        params.insert("agent_signature".into(), json!(agent_signature));
    }
    params.insert("correlation_id".into(), json!(correlation_id));

    let outcome =
        lower_and_dispatch_execute(&state.store, "api-host-pair", "host.pair", params).await?;
    let outcome = outcome.into_host_pair()?;

    let response = match outcome {
        HostPairOutcome::Paired { pairing_session_id } => HostPairResponse::Paired {
            correlation_id,
            pairing_session_id,
        },
        HostPairOutcome::Failed {
            reason_code,
            reason_detail,
            phase,
        } => HostPairResponse::Failed {
            correlation_id,
            reason_code,
            reason_detail,
            phase,
        },
    };

    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct OutboundSendRequest {
    contact_id: Uuid,
    campaign_id: Uuid,
    channel: String,
    #[serde(default)]
    correlation_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum OutboundSendResponse {
    Sent {
        correlation_id: Uuid,
        provider: &'static str,
        provider_message_id: String,
        channel: String,
    },
    Rejected {
        correlation_id: Uuid,
        reason_code: String,
        detail: String,
    },
    ProviderError {
        correlation_id: Uuid,
        reason_code: String,
        detail: String,
    },
}

/// Constitutional outbound.send proving surface. See
/// `docs/integration/huge-into-grammar.md` §4.
///
/// Accepts the minimal input — contact/campaign/channel plus an optional
/// `correlation_id` — and returns the terminal outcome with the binding
/// correlation_id so callers can audit the full chain in the evidence ledger.
async fn outbound_send(
    State(state): State<AppState>,
    Json(body): Json<OutboundSendRequest>,
) -> Result<Json<OutboundSendResponse>, ApiError> {
    let correlation_id = body.correlation_id.unwrap_or_else(Uuid::new_v4);
    let mut params = BTreeMap::new();
    params.insert("contact_id".into(), json!(body.contact_id));
    params.insert("campaign_id".into(), json!(body.campaign_id));
    params.insert("channel".into(), json!(body.channel));
    params.insert("correlation_id".into(), json!(correlation_id));

    let outcome =
        lower_and_dispatch_execute(&state.store, "api-outbound-send", "outbound.send", params)
            .await?;
    let outcome = outcome.into_outbound_send()?;

    let response = match outcome {
        OutboundSendOutcome::Sent {
            provider,
            provider_message_id,
            channel,
        } => OutboundSendResponse::Sent {
            correlation_id,
            provider,
            provider_message_id,
            channel,
        },
        OutboundSendOutcome::Rejected {
            reason_code,
            detail,
        } => OutboundSendResponse::Rejected {
            correlation_id,
            reason_code,
            detail,
        },
        OutboundSendOutcome::ProviderError {
            reason_code,
            detail,
        } => OutboundSendResponse::ProviderError {
            correlation_id,
            reason_code,
            detail,
        },
    };

    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct InstallReconcileRequest {
    host_id: Uuid,
    desired_manifest: serde_json::Value,
    #[serde(default)]
    applied_manifest: Option<serde_json::Value>,
    #[serde(default)]
    correlation_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum InstallReconcileResponse {
    Reconciled {
        correlation_id: Uuid,
        desired_hash: String,
        applied_steps: usize,
        skipped_steps: usize,
    },
    Failed {
        correlation_id: Uuid,
        desired_hash: String,
        reason_code: String,
        reason_detail: String,
        phase: &'static str,
        applied_steps: usize,
    },
}

/// Constitutional `install.reconcile` proving surface. See
/// `docs/integration/reconcile-anatomy.md`.
async fn install_reconcile(
    State(state): State<AppState>,
    axum::extract::Path(installation_id): axum::extract::Path<Uuid>,
    Json(body): Json<InstallReconcileRequest>,
) -> Result<Json<InstallReconcileResponse>, ApiError> {
    let correlation_id = body.correlation_id.unwrap_or_else(Uuid::new_v4);
    let mut params = BTreeMap::new();
    params.insert("installation_id".into(), json!(installation_id));
    params.insert("host_id".into(), json!(body.host_id));
    params.insert("desired_manifest".into(), body.desired_manifest);
    if let Some(applied_manifest) = body.applied_manifest {
        params.insert("applied_manifest".into(), applied_manifest);
    }
    params.insert("correlation_id".into(), json!(correlation_id));

    let outcome = lower_and_dispatch_execute(
        &state.store,
        "api-install-reconcile",
        "install.reconcile",
        params,
    )
    .await?;
    let outcome = outcome.into_install_reconcile()?;

    let response = match outcome {
        InstallReconcileOutcome::Reconciled {
            desired_hash,
            applied_steps,
            skipped_steps,
        } => InstallReconcileResponse::Reconciled {
            correlation_id,
            desired_hash,
            applied_steps,
            skipped_steps,
        },
        InstallReconcileOutcome::Failed {
            desired_hash,
            reason_code,
            reason_detail,
            phase,
            applied_steps,
        } => InstallReconcileResponse::Failed {
            correlation_id,
            desired_hash,
            reason_code,
            reason_detail,
            phase,
            applied_steps,
        },
    };

    Ok(Json(response))
}

/// Replay and explain one correlation from the append-only evidence ledger.
async fn evidence_trail(
    State(state): State<AppState>,
    axum::extract::Path(correlation_id): axum::extract::Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let trail = export_evidence_trail(&state.store, correlation_id).await?;
    Ok(Json(trail))
}

async fn healthz() -> impl IntoResponse {
    Json(HealthResponse {
        service: "minilab-api",
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn readyz(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    check_supabase_readiness(&state.store).await?;
    Ok(Json(ReadinessResponse {
        service: "minilab-api",
        status: "ready",
        sim_mode: state.store.sim_mode.as_str().into(),
    }))
}

async fn observability_metrics(
    State(state): State<AppState>,
) -> Result<Json<ObservabilityMetrics>, ApiError> {
    Ok(Json(build_observability_metrics(&state)?))
}

async fn operator_dashboard(
    State(state): State<AppState>,
) -> Result<Json<OperatorDashboard>, ApiError> {
    let mut sessions = state.agent_runtime.list_sessions()?;
    sessions.sort_by(|a, b| b.run_id.cmp(&a.run_id));
    sessions.truncate(20);
    Ok(Json(OperatorDashboard {
        metrics: build_observability_metrics(&state)?,
        recent_agent_sessions: sessions,
    }))
}

fn build_observability_metrics(state: &AppState) -> Result<ObservabilityMetrics, ApiError> {
    let sessions = state.agent_runtime.list_sessions()?;
    Ok(ObservabilityMetrics {
        service: "minilab-api".into(),
        sim_mode: state.store.sim_mode.as_str().into(),
        sim_branch_id: state.store.sim_branch.branch_id.clone(),
        sim_parent_branch_id: state.store.sim_branch.parent_branch_id.clone(),
        agent_sessions_total: sessions.len(),
        agent_sessions_pending: sessions.iter().filter(|session| session.pending).count(),
        evidence_close_span: "evidence.close",
    })
}

async fn twilio_whatsapp_webhook(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let signature = required_header(&headers, "x-twilio-signature")?;
    let auth_token = state.config.twilio_auth_token.as_deref().ok_or_else(|| {
        ApiError::misconfigured("TWILIO_AUTH_TOKEN is required for Twilio webhooks")
    })?;
    let webhook_url = external_request_url(&state.config, &headers, &uri)?;
    let form_params: BTreeMap<String, String> = serde_urlencoded::from_bytes(&body)
        .map_err(|err| ApiError::bad_request(format!("invalid Twilio form body: {err}")))?;

    if !validate_twilio_signature(auth_token, &webhook_url, &form_params, &signature)? {
        return Err(ApiError::unauthorized("invalid Twilio webhook signature"));
    }

    let payload: TwilioWhatsAppInboundPayload = serde_urlencoded::from_bytes(&body)
        .map_err(|err| ApiError::bad_request(format!("invalid Twilio WhatsApp payload: {err}")))?;

    ingest_reply(
        &state.store,
        IngestReplyInput {
            provider: "twilio_whatsapp".into(),
            payload: serde_json::to_value(payload).map_err(|err| {
                ApiError::bad_request(format!("failed to normalize Twilio payload: {err}"))
            })?,
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn sendgrid_parse_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    let signature = required_header(&headers, "x-twilio-email-event-webhook-signature")?;
    let timestamp = required_header(&headers, "x-twilio-email-event-webhook-timestamp")?;
    let public_key = state
        .config
        .sendgrid_parse_public_key
        .as_deref()
        .ok_or_else(|| {
            ApiError::misconfigured(
                "SENDGRID_PARSE_PUBLIC_KEY is required for SendGrid Parse webhooks",
            )
        })?;

    if !validate_sendgrid_signature(public_key, &timestamp, &body, &signature)? {
        return Err(ApiError::unauthorized(
            "invalid SendGrid Parse webhook signature",
        ));
    }

    let payload = parse_sendgrid_payload(&headers, body).await?;
    ingest_reply(
        &state.store,
        IngestReplyInput {
            provider: "twilio_sendgrid_parse".into(),
            payload: serde_json::to_value(payload).map_err(|err| {
                ApiError::bad_request(format!("failed to normalize SendGrid payload: {err}"))
            })?,
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn parse_sendgrid_payload(
    headers: &HeaderMap,
    body: Bytes,
) -> Result<SendGridParsePayload, ApiError> {
    let content_type = required_header(headers, CONTENT_TYPE.as_str())?;
    let boundary = multer::parse_boundary(&content_type)
        .map_err(|err| ApiError::bad_request(format!("invalid multipart boundary: {err}")))?;

    let stream = stream::once(async move { Ok::<Bytes, std::convert::Infallible>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut values = BTreeMap::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("invalid multipart field: {err}")))?
    {
        let Some(name) = field.name().map(str::to_string) else {
            continue;
        };

        match name.as_str() {
            "from" | "to" | "subject" | "text" | "html" | "headers" => {
                let value = field.text().await.map_err(|err| {
                    ApiError::bad_request(format!("invalid multipart text field `{name}`: {err}"))
                })?;
                values.insert(name, value);
            }
            _ => {
                let _ = field.bytes().await.map_err(|err| {
                    ApiError::bad_request(format!("invalid multipart field `{name}`: {err}"))
                })?;
            }
        }
    }

    Ok(SendGridParsePayload {
        from: values.remove("from").unwrap_or_default(),
        to: values.remove("to"),
        subject: values.remove("subject"),
        text: values.remove("text"),
        html: values.remove("html"),
        headers: values.remove("headers").unwrap_or_default(),
    })
}

async fn check_supabase_readiness(store: &StoreClient) -> Result<(), ApiError> {
    let response = store
        .http
        .get(store.rest("account"))
        .query(&[("select", "id"), ("limit", "1")])
        .send()
        .await
        .map_err(StoreError::from)?;

    let status = response.status().as_u16();
    if status >= 300 {
        let body = response.text().await.unwrap_or_default();
        return Err(ApiError::upstream(format!(
            "readiness probe failed with Supabase status {status}: {body}"
        )));
    }

    Ok(())
}

fn external_request_url(
    config: &ApiConfig,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<String, ApiError> {
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(uri.path());

    if let Some(base_url) = &config.public_base_url {
        return Ok(format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            path_and_query
        ));
    }

    if let Some((proto, host)) = forwarded_host_and_proto(headers) {
        return Ok(format!("{proto}://{host}{path_and_query}"));
    }

    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::misconfigured(
                "MINILAB_PUBLIC_BASE_URL or forwarded host headers are required for webhook validation",
            )
        })?;
    let proto = if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
        "http"
    } else {
        "https"
    };

    Ok(format!("{proto}://{host}{path_and_query}"))
}

fn forwarded_host_and_proto(headers: &HeaderMap) -> Option<(String, String)> {
    let forwarded = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok());
    if let Some(forwarded) = forwarded {
        let mut proto = None;
        let mut host = None;
        for part in forwarded.split(';') {
            let (key, value) = part.trim().split_once('=')?;
            let value = value.trim_matches('"');
            match key.trim().to_ascii_lowercase().as_str() {
                "proto" => proto = Some(value.to_string()),
                "host" => host = Some(value.to_string()),
                _ => {}
            }
        }
        if let (Some(proto), Some(host)) = (proto, host) {
            return Some((proto, host));
        }
    }

    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)?;
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(HOST))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)?;
    Some((proto, host))
}

fn required_header(headers: &HeaderMap, name: &str) -> Result<String, ApiError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| ApiError::bad_request(format!("missing required header `{name}`")))
}

async fn handle_middleware_error(error: BoxError) -> ApiError {
    if error.is::<tower::timeout::error::Elapsed>() {
        ApiError::timeout("request timed out")
    } else {
        ApiError::upstream(format!("middleware error: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use p256::{
        ecdsa::{signature::Signer, SigningKey},
        pkcs8::EncodePublicKey,
    };
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() {
        let app = build_app(test_state("http://127.0.0.1:9".into(), None, None));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_runtime_message_route_records_runtime_pipeline() {
        let app = build_app(test_state("http://127.0.0.1:9".into(), None, None));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agent-runtime/places/chatgpt_workspace/messages")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "text": "summarize the current runtime state",
                            "appId": "chatgpt_workspace_agent"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let ack: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(ack["output_kind"], "advisory");
        let session_id = ack["session_id"].as_str().expect("session id");

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/agent-runtime/sessions/{session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(snapshot["outputKind"], "advisory");
        assert_eq!(
            snapshot["runtimePipeline"]["candidateKind"],
            "strong_system_review"
        );
    }

    #[tokio::test]
    async fn agent_runtime_place_and_session_list_routes_are_mounted() {
        let app = build_app(test_state("http://127.0.0.1:9".into(), None, None));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/agent-runtime/places/chatgpt_workspace")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let profile: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(profile["execution_substrate"], "chatgpt_business");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/agent-runtime/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_command_route_is_mounted() {
        let app = build_app(test_state("http://127.0.0.1:9".into(), None, None));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp/command")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "initialize",
                            "params": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            payload["result"]["serverInfo"]["name"],
            "minilab-mcp-command"
        );
    }

    #[tokio::test]
    async fn evidence_trail_route_is_mounted() {
        let app = build_app(test_state("http://127.0.0.1:9".into(), None, None));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/evidence/00000000-0000-0000-0000-000000000000/trail")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn twilio_webhook_rejects_invalid_signature() {
        let app = build_app(test_state(
            "http://127.0.0.1:9".into(),
            Some("twilio-secret".into()),
            None,
        ));

        let body = "MessageSid=SM123&From=whatsapp%3A%2B15551234567&To=whatsapp%3A%2B15557654321&Body=hello";
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks/twilio/whatsapp")
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header("x-twilio-signature", "not-valid")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sendgrid_webhook_rejects_unreachable_upstream_after_valid_signature() {
        let signing_key = SigningKey::from_bytes((&[9u8; 32]).into()).unwrap();
        let public_key = STANDARD.encode(
            signing_key
                .verifying_key()
                .to_public_key_der()
                .unwrap()
                .as_ref(),
        );
        let app = build_app(test_state(
            "http://127.0.0.1:9".into(),
            None,
            Some(public_key),
        ));

        let boundary = "----minilab-boundary";
        let body = multipart_body(
            boundary,
            &[
                ("from", "Buyer <buyer@acme.example>"),
                (
                    "to",
                    "reply+00000000-0000-0000-0000-000000000000@minilab.example",
                ),
                ("subject", "Re: hello"),
                ("text", "interested"),
                ("headers", "Message-ID: <inbound@example.com>\n"),
            ],
        );
        let timestamp = "1700000000";
        let mut digest_input = Vec::new();
        digest_input.extend_from_slice(timestamp.as_bytes());
        digest_input.extend_from_slice(body.as_bytes());
        let digest = Sha256::digest(&digest_input);
        let signature: p256::ecdsa::Signature = signing_key.sign(&digest);
        let signature = STANDARD.encode(signature.to_der().as_bytes());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks/sendgrid/parse")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header("x-twilio-email-event-webhook-signature", signature)
                    .header("x-twilio-email-event-webhook-timestamp", timestamp)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn parse_sendgrid_payload_extracts_reply_fields() {
        let boundary = "----minilab-boundary";
        let body = multipart_body(
            boundary,
            &[
                ("from", "Buyer <buyer@acme.example>"),
                (
                    "to",
                    "reply+00000000-0000-0000-0000-000000000000@minilab.example",
                ),
                ("subject", "Re: hello"),
                ("text", "interested"),
                ("headers", "Message-ID: <inbound@example.com>\n"),
            ],
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}")
                .parse()
                .unwrap(),
        );

        let payload = parse_sendgrid_payload(&headers, Bytes::from(body))
            .await
            .unwrap();
        assert_eq!(payload.from, "Buyer <buyer@acme.example>");
        assert_eq!(
            payload.to.as_deref(),
            Some("reply+00000000-0000-0000-0000-000000000000@minilab.example")
        );
        assert_eq!(payload.subject.as_deref(), Some("Re: hello"));
    }

    fn test_state(
        base_url: String,
        twilio_auth_token: Option<String>,
        sendgrid_parse_public_key: Option<String>,
    ) -> AppState {
        AppState {
            store: StoreClient::new(base_url, "service-key"),
            agent_runtime: AgentRuntimeService::new().expect("agent runtime service"),
            config: Arc::new(ApiConfig {
                bind_addr: "127.0.0.1:3000".parse().unwrap(),
                public_base_url: Some("https://api.minilab.example".into()),
                request_timeout: std::time::Duration::from_secs(5),
                twilio_max_body_bytes: 262_144,
                sendgrid_max_body_bytes: 1024 * 1024,
                twilio_auth_token,
                sendgrid_parse_public_key,
            }),
        }
    }

    fn multipart_body(boundary: &str, fields: &[(&str, &str)]) -> String {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        body
    }
}
