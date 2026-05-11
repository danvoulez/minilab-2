use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{app::AppState, error::ApiError};

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const SERVER_NAME: &str = "minilab-mcp-artifacts";
const SERVER_VERSION: &str = "0.1.0";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/artifacts", post(handle_mcp_request))
        .route("/artifacts/healthz", get(healthz))
}

async fn healthz() -> impl IntoResponse {
    Json(json!({
        "service": SERVER_NAME,
        "status": "ok",
        "protocol": "mcp",
        "transport": "streamable_http"
    }))
}

async fn handle_mcp_request(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, ApiError> {
    let request: JsonRpcRequest = serde_json::from_value(payload)
        .map_err(|err| ApiError::bad_request(format!("invalid MCP JSON-RPC payload: {err}")))?;

    if request.id.is_none() {
        if request.method == "notifications/initialized" {
            return Ok(StatusCode::ACCEPTED.into_response());
        }
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let response = dispatch_request(&state, request)
        .map_err(|err| ApiError::bad_request(format!("mcp artifacts request failed: {err}")))?;
    Ok(Json(response).into_response())
}

fn dispatch_request(state: &AppState, request: JsonRpcRequest) -> Result<JsonRpcResponse, String> {
    let id = request.id.unwrap_or_else(|| json!(null));
    match request.method.as_str() {
        "initialize" => Ok(JsonRpcResponse::result(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            }),
        )),
        "ping" => Ok(JsonRpcResponse::result(id, json!({}))),
        "tools/list" => Ok(JsonRpcResponse::result(
            id,
            json!({
                "tools": [
                    tool_definition(
                        "create_artifact",
                        "Create an official artifact record attached to a Minilab session or run. This never writes canonical truth directly.",
                        json!({
                            "type": "object",
                            "properties": {
                                "session_id": { "type": "string" },
                                "run_id": { "type": "string" },
                                "artifact_kind": { "type": "string" },
                                "title": { "type": "string" },
                                "mime_type": { "type": "string" },
                                "content_ref": { "type": "string" },
                                "inline_content": { "type": "string" },
                                "source_place": { "type": "string" },
                                "source_system": { "type": "string" },
                                "metadata": { "type": "object" }
                            },
                            "required": ["artifact_kind", "title", "mime_type", "source_place", "source_system"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "attach_chatgpt_summary",
                        "Attach an official external summary to a run and optionally reference artifact ids.",
                        json!({
                            "type": "object",
                            "properties": {
                                "run_id": { "type": "string" },
                                "summary": { "type": "string" },
                                "artifact_refs": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["run_id", "summary"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "attach_run_output",
                        "Attach the official output class and payload for a run. Only closed normalization classes are accepted.",
                        json!({
                            "type": "object",
                            "properties": {
                                "run_id": { "type": "string" },
                                "output_class": { "type": "string", "enum": ["advisory", "proposal", "artifact", "execution_summary", "handoff_result"] },
                                "payload": { "type": "object" },
                                "summary": { "type": "string" },
                                "artifact_refs": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["run_id", "output_class", "payload", "summary"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "store_external_result",
                        "Store a structured external result against a run using a closed normalization class.",
                        json!({
                            "type": "object",
                            "properties": {
                                "run_id": { "type": "string" },
                                "result_kind": { "type": "string" },
                                "content": { "type": "object" },
                                "summary": { "type": "string" },
                                "source_place": { "type": "string" },
                                "normalization_class": { "type": "string", "enum": ["advisory", "proposal", "artifact", "execution_summary", "handoff_result"] }
                            },
                            "required": ["run_id", "result_kind", "content", "summary", "source_place", "normalization_class"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "append_evidence_note",
                        "Append an evidential note to a session or run without mutating canonical truth.",
                        json!({
                            "type": "object",
                            "properties": {
                                "session_id": { "type": "string" },
                                "run_id": { "type": "string" },
                                "note": { "type": "string" },
                                "kind": { "type": "string" }
                            },
                            "required": ["note"],
                            "additionalProperties": false
                        })
                    )
                ]
            }),
        )),
        "tools/call" => {
            let params = request
                .params
                .ok_or_else(|| "tools/call requires params".to_string())?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "tools/call requires a string params.name".to_string())?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = call_tool(state, name, arguments)?;
            Ok(JsonRpcResponse::result(id, result))
        }
        other => Ok(JsonRpcResponse::error(
            id,
            -32601,
            format!("method not found: {other}"),
        )),
    }
}

fn call_tool(state: &AppState, name: &str, arguments: Value) -> Result<Value, String> {
    match name {
        "create_artifact" => {
            let session_id = optional_string(&arguments, "session_id");
            let run_id = optional_string(&arguments, "run_id");
            let artifact_kind = required_string(&arguments, "artifact_kind")?;
            let title = required_string(&arguments, "title")?;
            let mime_type = required_string(&arguments, "mime_type")?;
            let content_ref = optional_string(&arguments, "content_ref").map(str::to_string);
            let inline_content = optional_string(&arguments, "inline_content").map(str::to_string);
            let source_place = required_string(&arguments, "source_place")?;
            let source_system = required_string(&arguments, "source_system")?;
            let metadata = optional_object(&arguments, "metadata").cloned();
            let result = state
                .agent_runtime
                .create_artifact(
                    session_id,
                    run_id,
                    artifact_kind,
                    title,
                    mime_type,
                    content_ref,
                    inline_content,
                    source_place,
                    source_system,
                    metadata,
                )
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        "attach_chatgpt_summary" => {
            let run_id = required_string(&arguments, "run_id")?;
            let summary = required_string(&arguments, "summary")?;
            let artifact_refs = string_array(&arguments, "artifact_refs");
            let result = state
                .agent_runtime
                .attach_chatgpt_summary(run_id, summary, artifact_refs)
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        "attach_run_output" => {
            let run_id = required_string(&arguments, "run_id")?;
            let output_class = required_string(&arguments, "output_class")?;
            let payload = required_object(&arguments, "payload")?.clone();
            let summary = required_string(&arguments, "summary")?;
            let artifact_refs = string_array(&arguments, "artifact_refs");
            let result = state
                .agent_runtime
                .attach_run_output(run_id, output_class, payload, summary, artifact_refs)
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        "store_external_result" => {
            let run_id = required_string(&arguments, "run_id")?;
            let result_kind = required_string(&arguments, "result_kind")?;
            let content = required_object(&arguments, "content")?.clone();
            let summary = required_string(&arguments, "summary")?;
            let source_place = required_string(&arguments, "source_place")?;
            let normalization_class = required_string(&arguments, "normalization_class")?;
            let result = state
                .agent_runtime
                .store_external_result(
                    run_id,
                    result_kind,
                    content,
                    summary,
                    source_place,
                    normalization_class,
                )
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        "append_evidence_note" => {
            let session_id = optional_string(&arguments, "session_id");
            let run_id = optional_string(&arguments, "run_id");
            let note = required_string(&arguments, "note")?;
            let kind = optional_string(&arguments, "kind");
            let result = state
                .agent_runtime
                .append_evidence_note(session_id, run_id, note, kind)
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn tool_definition(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn tool_result(value: Value, is_error: bool) -> Result<Value, String> {
    let text = serde_json::to_string_pretty(&value).map_err(|err| err.to_string())?;
    let mut result = json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": value
    });
    if is_error {
        result["isError"] = json!(true);
    }
    Ok(result)
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("missing required string argument `{key}`"))
}

fn optional_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
}

fn required_object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or_else(|| format!("missing required object argument `{key}`"))
}

fn optional_object<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| value.is_object())
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn api_error_to_string(err: ApiError) -> String {
    err.to_string()
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn result(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{agent_runtime::AgentRuntimeService, app::AppState, config::ApiConfig};
    use minilab_store::StoreClient;

    fn test_state() -> AppState {
        AppState {
            store: StoreClient::new("https://example.supabase.co", "service-key"),
            agent_runtime: AgentRuntimeService::new().expect("agent runtime service"),
            config: Arc::new(ApiConfig {
                bind_addr: "127.0.0.1:3000".parse().unwrap(),
                public_base_url: Some("https://api.minilab.example".into()),
                request_timeout: std::time::Duration::from_secs(5),
                twilio_max_body_bytes: 262_144,
                sendgrid_max_body_bytes: 1024 * 1024,
                twilio_auth_token: None,
                sendgrid_parse_public_key: None,
            }),
        }
    }

    fn open_run(state: &AppState) -> (String, String) {
        let ack = state
            .agent_runtime
            .submit_place_intent(
                "chatgpt_workspace",
                None,
                "drift_review".into(),
                json!({ "target": "runtime", "window": "24h" }),
            )
            .expect("place intent should succeed");
        (ack.session_id, ack.run_id)
    }

    #[test]
    fn tools_list_exposes_artifact_tools() {
        let state = test_state();
        let response = dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/list".into(),
                params: None,
            },
        )
        .expect("tools/list should succeed");
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|tool| tool["name"] == "create_artifact"));
        assert!(tools.iter().any(|tool| tool["name"] == "attach_run_output"));
    }

    #[test]
    fn attach_run_output_updates_snapshot_visible_to_runtime() {
        let state = test_state();
        let (session_id, run_id) = open_run(&state);
        dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(2)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "attach_run_output",
                    "arguments": {
                        "run_id": run_id,
                        "output_class": "execution_summary",
                        "payload": { "result": "ok" },
                        "summary": "External run finished well.",
                        "artifact_refs": ["artifact-123"]
                    }
                })),
            },
        )
        .expect("tools/call should succeed");

        let snapshot = state
            .agent_runtime
            .get_session(&session_id)
            .expect("read should succeed")
            .expect("session should exist");
        assert_eq!(snapshot.output_kind.as_deref(), Some("execution_summary"));
        assert_eq!(snapshot.artifacts.as_ref().map(|a| a.count), Some(1));
        assert_eq!(
            snapshot
                .checkpoint
                .as_ref()
                .and_then(|c| c.summary.as_deref()),
            Some("run_output_attached:execution_summary")
        );
    }

    #[test]
    fn create_artifact_and_summary_close_the_loop() {
        let state = test_state();
        let (session_id, run_id) = open_run(&state);

        dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(3)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "create_artifact",
                    "arguments": {
                        "run_id": run_id,
                        "artifact_kind": "report",
                        "title": "Weekly summary",
                        "mime_type": "text/markdown",
                        "inline_content": "# summary",
                        "source_place": "chatgpt_workspace",
                        "source_system": "chatgpt_workspace"
                    }
                })),
            },
        )
        .expect("create_artifact should succeed");

        dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(4)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "attach_chatgpt_summary",
                    "arguments": {
                        "run_id": run_id,
                        "summary": "ChatGPT attached an external summary.",
                        "artifact_refs": ["report:Weekly summary"]
                    }
                })),
            },
        )
        .expect("attach_chatgpt_summary should succeed");

        let snapshot = state
            .agent_runtime
            .get_session(&session_id)
            .expect("read should succeed")
            .expect("session should exist");
        assert_eq!(
            snapshot.reply_text.as_deref(),
            Some("ChatGPT attached an external summary.")
        );
        assert_eq!(snapshot.artifacts.as_ref().map(|a| a.count), Some(2));
    }
}
