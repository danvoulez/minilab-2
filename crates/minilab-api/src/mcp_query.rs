use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{agent_runtime::AgentRuntimeService, app::AppState, error::ApiError};

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const SERVER_NAME: &str = "minilab-mcp-query";
const SERVER_VERSION: &str = "0.1.0";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/query", post(handle_mcp_request))
        .route("/query/healthz", get(healthz))
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

    let response = dispatch_request(&state.agent_runtime, request)
        .map_err(|err| ApiError::bad_request(format!("mcp query request failed: {err}")))?;
    Ok(Json(response).into_response())
}

fn dispatch_request(
    service: &AgentRuntimeService,
    request: JsonRpcRequest,
) -> Result<JsonRpcResponse, String> {
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
                        "get_place_status",
                        "Get the current status and runtime shell summary for a Minilab place.",
                        json!({
                            "type": "object",
                            "properties": {
                                "place_id": { "type": "string", "description": "Canonical place id or slug." }
                            },
                            "required": ["place_id"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "get_place_context",
                        "Get the place profile, output policy, and adapter defaults for a Minilab place.",
                        json!({
                            "type": "object",
                            "properties": {
                                "place_id": { "type": "string", "description": "Canonical place id or slug." }
                            },
                            "required": ["place_id"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "get_agent_session",
                        "Read an official Minilab agent session snapshot by session id.",
                        json!({
                            "type": "object",
                            "properties": {
                                "session_id": { "type": "string", "description": "Minilab agent session id." }
                            },
                            "required": ["session_id"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "get_recent_timeline",
                        "Get recent audit events for a Minilab agent session or place.",
                        json!({
                            "type": "object",
                            "properties": {
                                "session_id": { "type": "string" },
                                "place_id": { "type": "string" },
                                "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
                            },
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "get_terminal_session",
                        "Read a terminal session attached to a Minilab agent session, if present.",
                        json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Terminal session id." }
                            },
                            "required": ["id"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "search_runtime_docs",
                        "Search Minilab runtime docs by keyword.",
                        json!({
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Keyword query." },
                                "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
                            },
                            "required": ["query"],
                            "additionalProperties": false
                        })
                    ),
                    tool_definition(
                        "fetch_runtime_doc",
                        "Fetch a runtime doc by id returned from search_runtime_docs.",
                        json!({
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Doc id, usually a repo-relative path under docs/." }
                            },
                            "required": ["id"],
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
            let result = call_tool(service, name, arguments)?;
            Ok(JsonRpcResponse::result(id, result))
        }
        other => Ok(JsonRpcResponse::error(
            id,
            -32601,
            format!("method not found: {other}"),
        )),
    }
}

fn call_tool(service: &AgentRuntimeService, name: &str, arguments: Value) -> Result<Value, String> {
    match name {
        "get_place_status" => {
            let place_id = required_string(&arguments, "place_id")?;
            let profile = service
                .resolve_profile(place_id)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("unknown place `{place_id}`"))?;
            let sessions = service.list_sessions().map_err(|err| err.to_string())?;
            let session_count = sessions
                .iter()
                .filter(|session| {
                    session.place_id.as_deref() == Some(profile.place_id.as_str())
                        || session.place_id.as_deref() == Some(profile.slug.as_str())
                })
                .count();
            tool_result(json!({
                "place_id": profile.place_id,
                "slug": profile.slug,
                "label": profile.label,
                "mode": profile.mode,
                "execution_mode": profile.execution_mode,
                "execution_substrate": profile.execution_substrate,
                "session_count": session_count,
                "status": if session_count > 0 { "active" } else { "idle" }
            }))
        }
        "get_place_context" => {
            let place_id = required_string(&arguments, "place_id")?;
            let profile = service
                .resolve_profile(place_id)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("unknown place `{place_id}`"))?;
            tool_result(json!(profile))
        }
        "get_agent_session" => {
            let session_id = required_string(&arguments, "session_id")?;
            let snapshot = service
                .get_session(session_id)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| format!("unknown session `{session_id}`"))?;
            tool_result(json!(snapshot))
        }
        "get_recent_timeline" => {
            let limit = optional_usize(&arguments, "limit")
                .unwrap_or(5)
                .clamp(1, 20);
            let session_id = optional_string(&arguments, "session_id");
            let place_id = optional_string(&arguments, "place_id");
            let sessions = service.list_sessions().map_err(|err| err.to_string())?;
            let mut events = Vec::new();

            for session in sessions {
                let session_matches = session_id
                    .as_deref()
                    .map(|id| session.session_id == id)
                    .unwrap_or(true);
                let place_matches = place_id
                    .as_deref()
                    .map(|id| session.place_id.as_deref() == Some(id))
                    .unwrap_or(true);
                if !session_matches || !place_matches {
                    continue;
                }
                if let Some(audit) = session.audit_trail.as_ref() {
                    for event in &audit.recent_events {
                        events.push(json!({
                            "session_id": session.session_id,
                            "place_id": session.place_id,
                            "kind": event.kind,
                            "summary": event.summary
                        }));
                    }
                }
            }

            events.truncate(limit);
            tool_result(json!({ "events": events }))
        }
        "get_terminal_session" => {
            let terminal_id = required_string(&arguments, "id")?;
            let sessions = service.list_sessions().map_err(|err| err.to_string())?;
            let terminal = sessions
                .into_iter()
                .find_map(|session| {
                    session.terminal_session.and_then(|terminal| {
                        if terminal.id == terminal_id {
                            Some(json!({
                                "session_id": session.session_id,
                                "place_id": session.place_id,
                                "terminal": terminal
                            }))
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| {
                    json!({
                        "found": false,
                        "id": terminal_id,
                        "reason": "terminal_sync_not_implemented"
                    })
                });
            tool_result(terminal)
        }
        "search_runtime_docs" => {
            let query = required_string(&arguments, "query")?;
            let limit = optional_usize(&arguments, "limit")
                .unwrap_or(5)
                .clamp(1, 20);
            let results = search_runtime_docs(query, limit).map_err(|err| err.to_string())?;
            tool_result(json!({ "results": results }))
        }
        "fetch_runtime_doc" => {
            let id = required_string(&arguments, "id")?;
            let doc = fetch_runtime_doc(id).map_err(|err| err.to_string())?;
            tool_result(doc)
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn tool_definition(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": true
        }
    })
}

fn tool_result(value: Value) -> Result<Value, String> {
    let text = serde_json::to_string_pretty(&value).map_err(|err| err.to_string())?;
    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": value
    }))
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

fn optional_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
}

fn docs_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn search_runtime_docs(query: &str, limit: usize) -> Result<Vec<Value>, std::io::Error> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    let mut results = Vec::new();
    let root = docs_root();
    let mut files = Vec::new();
    collect_markdown_files(&root, &mut files)?;

    for file in files {
        let content = fs::read_to_string(&file)?;
        let haystack = content.to_lowercase();
        if !tokens.iter().all(|token| haystack.contains(token)) {
            continue;
        }
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file.as_path())
            .to_string_lossy()
            .to_string();
        let title = first_heading(&content).unwrap_or_else(|| rel.clone());
        let snippet = content
            .lines()
            .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .unwrap_or("")
            .trim()
            .chars()
            .take(180)
            .collect::<String>();
        results.push(json!({
            "id": rel,
            "title": title,
            "url": format!("docs/{}", rel),
            "snippet": snippet
        }));
        if results.len() >= limit {
            break;
        }
    }

    Ok(results)
}

fn fetch_runtime_doc(id: &str) -> Result<Value, std::io::Error> {
    let root = docs_root();
    let path = root.join(id);
    let content = fs::read_to_string(&path)?;
    let title = first_heading(&content).unwrap_or_else(|| id.to_string());
    Ok(json!({
        "id": id,
        "title": title,
        "url": format!("docs/{}", id),
        "content": content
    }))
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn first_heading(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|value| value.trim().to_string())
    })
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
    use super::*;
    use crate::agent_runtime::{AgentRuntimeFileRef, AgentRuntimeSendRequest};

    #[test]
    fn initialize_returns_tools_capability() {
        let service = AgentRuntimeService::new().expect("service");
        let response = dispatch_request(
            &service,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "initialize".into(),
                params: Some(json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "0.1.0" }
                })),
            },
        )
        .expect("initialize should succeed");
        assert_eq!(
            response.result.unwrap()["capabilities"]["tools"]["listChanged"],
            json!(false)
        );
    }

    #[test]
    fn tools_list_exposes_query_tools() {
        let service = AgentRuntimeService::new().expect("service");
        let response = dispatch_request(
            &service,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(2)),
                method: "tools/list".into(),
                params: None,
            },
        )
        .expect("tools/list should succeed");
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|tool| tool["name"] == "get_place_status"));
        assert!(tools
            .iter()
            .any(|tool| tool["name"] == "search_runtime_docs"));
    }

    #[test]
    fn get_agent_session_reads_runtime_shell_snapshot() {
        let service = AgentRuntimeService::new().expect("service");
        let ack = service
            .submit_message(
                "chatgpt_workspace",
                AgentRuntimeSendRequest {
                    session_id: None,
                    text: "summarize current runtime state".into(),
                    app_id: None,
                    policy_overrides: None,
                    files: vec![AgentRuntimeFileRef {
                        name: "note.txt".into(),
                        size_bytes: 12,
                        mime_type: Some("text/plain".into()),
                    }],
                },
            )
            .expect("send");

        let response = dispatch_request(
            &service,
            JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(3)),
                method: "tools/call".into(),
                params: Some(json!({
                    "name": "get_agent_session",
                    "arguments": {
                        "session_id": ack.session_id
                    }
                })),
            },
        )
        .expect("tools/call should succeed");
        let structured = response.result.unwrap()["structuredContent"].clone();
        assert_eq!(structured["placeId"], json!("chatgpt_workspace"));
        assert_eq!(structured["outputKind"], json!("artifact"));
    }
}
