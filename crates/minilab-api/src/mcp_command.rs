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
const SERVER_NAME: &str = "minilab-mcp-command";
const SERVER_VERSION: &str = "0.1.0";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/command", post(handle_mcp_request))
        .route("/command/healthz", get(healthz))
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
        .map_err(|err| ApiError::bad_request(format!("mcp command request failed: {err}")))?;
    Ok(Json(response).into_response())
}

fn dispatch_request(state: &AppState, request: JsonRpcRequest) -> Result<JsonRpcResponse, String> {
    let id = request.id.unwrap_or(json!(null));
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
                        "submit_place_intent",
                        "Submit a structured place intent into Minilab's governed boundary. This does not directly execute a material action.",
                        json!({
                            "type": "object",
                            "properties": {
                                "place_id": { "type": "string" },
                                "session_id": { "type": "string" },
                                "intent_kind": { "type": "string" },
                                "payload": { "type": "object" }
                            },
                            "required": ["place_id", "intent_kind", "payload"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "start_governed_handoff",
                        "Open a governed handoff inside Minilab. The result is a place-shaped handoff record, not direct execution.",
                        json!({
                            "type": "object",
                            "properties": {
                                "place_id": { "type": "string" },
                                "session_id": { "type": "string" },
                                "task_kind": { "type": "string" },
                                "payload": { "type": "object" }
                            },
                            "required": ["place_id", "task_kind", "payload"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "continue_terminal_session",
                        "Request governed continuation of a terminal session. This does not bypass Minilab-side confirmation.",
                        json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "command": { "type": "string" }
                            },
                            "required": ["id", "command"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "request_confirmation",
                        "Request Minilab-side confirmation for a previously staged governed action.",
                        json!({
                            "type": "object",
                            "properties": {
                                "action_ref": { "type": "string" }
                            },
                            "required": ["action_ref"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "submit_operational_action",
                        "Stage an operational action through Minilab's governed boundary. This creates a governed proposal and does not directly execute.",
                        json!({
                            "type": "object",
                            "properties": {
                                "place_id": { "type": "string" },
                                "session_id": { "type": "string" },
                                "action_kind": { "type": "string" },
                                "params": { "type": "object" },
                                "policy_context": { "type": "object" }
                            },
                            "required": ["place_id", "action_kind", "params"],
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
        "submit_place_intent" => {
            let place_id = required_string(&arguments, "place_id")?;
            let session_id = optional_string(&arguments, "session_id").map(str::to_string);
            let intent_kind = required_string(&arguments, "intent_kind")?.to_string();
            let payload = required_object(&arguments, "payload")?.clone();
            let ack = state
                .agent_runtime
                .submit_place_intent(place_id, session_id, intent_kind, payload)
                .map_err(api_error_to_string)?;
            tool_result(json!(ack), false)
        }
        "start_governed_handoff" => {
            let place_id = required_string(&arguments, "place_id")?;
            let session_id = optional_string(&arguments, "session_id").map(str::to_string);
            let task_kind = required_string(&arguments, "task_kind")?.to_string();
            let payload = required_object(&arguments, "payload")?.clone();
            let ack = state
                .agent_runtime
                .start_governed_handoff(place_id, session_id, task_kind, payload)
                .map_err(api_error_to_string)?;
            tool_result(json!(ack), false)
        }
        "continue_terminal_session" => {
            let id = required_string(&arguments, "id")?;
            let command = required_string(&arguments, "command")?;
            let result = state
                .agent_runtime
                .continue_terminal_session(id, command)
                .map_err(api_error_to_string)?;
            let is_error = result["status"] == json!("unavailable");
            tool_result(result, is_error)
        }
        "request_confirmation" => {
            let action_ref = required_string(&arguments, "action_ref")?;
            let result = state
                .agent_runtime
                .request_confirmation(action_ref)
                .map_err(api_error_to_string)?;
            tool_result(result, false)
        }
        "submit_operational_action" => {
            let place_id = required_string(&arguments, "place_id")?;
            let session_id = optional_string(&arguments, "session_id").map(str::to_string);
            let action_kind = required_string(&arguments, "action_kind")?.to_string();
            let params = required_object(&arguments, "params")?.clone();
            let policy_context = optional_object(&arguments, "policy_context").cloned();
            let ack = state
                .agent_runtime
                .submit_operational_action(
                    place_id,
                    session_id,
                    action_kind,
                    params,
                    policy_context,
                )
                .map_err(api_error_to_string)?;
            tool_result(json!(ack), false)
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

    #[test]
    fn tools_list_exposes_command_tools() {
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
        assert!(tools
            .iter()
            .any(|tool| tool["name"] == "submit_place_intent"));
        assert!(tools
            .iter()
            .any(|tool| tool["name"] == "submit_operational_action"));
    }

    #[test]
    fn submit_place_intent_opens_waiting_place_session() {
        let state = test_state();
        let response = dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(2)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "submit_place_intent",
                    "arguments": {
                        "place_id": "chatgpt_workspace",
                        "intent_kind": "drift_review",
                        "payload": {
                            "target": "runtime",
                            "window": "24h"
                        }
                    }
                })),
            },
        )
        .expect("tools/call should succeed");
        let structured = response.result.unwrap()["structuredContent"].clone();
        assert_eq!(structured["output_kind"], json!("proposal"));
        assert_eq!(structured["status"], json!("waiting"));
    }

    #[test]
    fn continue_terminal_session_is_honest_when_terminal_sync_is_absent() {
        let state = test_state();
        let response = dispatch_request(
            &state,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(3)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "continue_terminal_session",
                    "arguments": {
                        "id": "term-123",
                        "command": "ls -la"
                    }
                })),
            },
        )
        .expect("tools/call should succeed");
        let result = response.result.unwrap();
        assert_eq!(result["isError"], json!(true));
        assert_eq!(
            result["structuredContent"]["reason"],
            json!("terminal_sync_not_implemented")
        );
    }
}
