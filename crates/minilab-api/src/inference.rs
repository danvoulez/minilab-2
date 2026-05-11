use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_runtime::PlaceProfile;

const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone)]
pub struct InferenceInput {
    pub profile: PlaceProfile,
    pub text: String,
    pub attachments: Vec<InferenceAttachment>,
}

#[derive(Debug, Clone)]
pub struct InferenceAttachment {
    pub name: String,
    pub size_bytes: usize,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputClass {
    Advisory,
    Proposal,
    Artifact,
    HandoffResult,
}

impl OutputClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputClass::Advisory => "advisory",
            OutputClass::Proposal => "proposal",
            OutputClass::Artifact => "artifact",
            OutputClass::HandoffResult => "handoff_result",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub action_kind: String,
    pub invocation_family: String,
    pub requires_confirmation: bool,
    pub outcome_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDraft {
    pub primary_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedModelOutput {
    pub output_class: OutputClass,
    pub acknowledgement: String,
    pub reply_text: String,
    pub checkpoint_summary: String,
    pub policy_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_action: Option<ProposedAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactDraft>,
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("inference backend is disabled: {0}")]
    Disabled(&'static str),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("openai returned {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("missing model output text")]
    MissingOutputText,
    #[error("missing ollama output text")]
    MissingOllamaOutputText,
    #[error("invalid model output json: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

#[async_trait]
trait Backend: Send + Sync {
    async fn respond(&self, input: InferenceInput) -> Result<NormalizedModelOutput, InferenceError>;
}

#[derive(Clone)]
pub struct InferenceClient {
    inner: Arc<dyn Backend>,
}

impl InferenceClient {
    pub fn from_env() -> Self {
        let backend = std::env::var("MINILAB_AGENT_BACKEND")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                if std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .is_some()
                {
                    "openai".into()
                } else {
                    "ollama".into()
                }
            });

        match backend.as_str() {
            "openai" => {
                let api_key = std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_default();
                if api_key.is_empty() {
                    return Self {
                        inner: Arc::new(DisabledBackend),
                    };
                }
                let model = std::env::var("MINILAB_AGENT_OPENAI_MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "gpt-4.1-mini".into());
                let timeout_secs = std::env::var("MINILAB_AGENT_OPENAI_TIMEOUT_SECS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(30);
                Self {
                    inner: Arc::new(
                        OpenAiResponsesBackend::new(api_key, model, Duration::from_secs(timeout_secs))
                            .expect("openai inference backend"),
                    ),
                }
            }
            "ollama" => {
                let model = std::env::var("MINILAB_AGENT_OLLAMA_MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "qwen2.5:7b-instruct".into());
                let base_url = std::env::var("MINILAB_AGENT_OLLAMA_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        std::env::var("OLLAMA_HOST")
                            .ok()
                            .filter(|value| !value.trim().is_empty())
                    })
                    .unwrap_or_else(|| DEFAULT_OLLAMA_URL.into());
                let timeout_secs = std::env::var("MINILAB_AGENT_OLLAMA_TIMEOUT_SECS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(60);
                Self {
                    inner: Arc::new(
                        OllamaBackend::new(base_url, model, Duration::from_secs(timeout_secs))
                            .expect("ollama inference backend"),
                    ),
                }
            }
            "disabled" => Self {
                inner: Arc::new(DisabledBackend),
            },
            _ => Self {
                inner: Arc::new(DisabledBackend),
            },
        }
    }

    pub async fn respond(
        &self,
        input: InferenceInput,
    ) -> Result<NormalizedModelOutput, InferenceError> {
        self.inner.respond(input).await
    }
}

struct DisabledBackend;

#[async_trait]
impl Backend for DisabledBackend {
    async fn respond(
        &self,
        _input: InferenceInput,
    ) -> Result<NormalizedModelOutput, InferenceError> {
        Err(InferenceError::Disabled(
            "set OPENAI_API_KEY to enable agent runtime inference",
        ))
    }
}

struct OllamaBackend {
    http: Client,
    base_url: String,
    model: String,
}

impl OllamaBackend {
    fn new(base_url: String, model: String, timeout: Duration) -> Result<Self, reqwest::Error> {
        let http = Client::builder().timeout(timeout).build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').into(),
            model,
        })
    }
}

#[async_trait]
impl Backend for OllamaBackend {
    async fn respond(&self, input: InferenceInput) -> Result<NormalizedModelOutput, InferenceError> {
        let request_body = json!({
            "model": self.model,
            "stream": false,
            "format": "json",
            "messages": [
                {
                    "role": "system",
                    "content": developer_instructions(&input.profile)
                },
                {
                    "role": "user",
                    "content": render_user_input(&input)
                }
            ]
        });
        let url = format!("{}/api/chat", self.base_url);
        let response = self.http.post(url).json(&request_body).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(InferenceError::Upstream {
                status: status.as_u16(),
                body,
            });
        }
        let value: Value = serde_json::from_str(&body)?;
        let text = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
            .ok_or(InferenceError::MissingOllamaOutputText)?;
        let output: NormalizedModelOutput = serde_json::from_str(&text)?;
        Ok(output)
    }
}

struct OpenAiResponsesBackend {
    http: Client,
    model: String,
}

impl OpenAiResponsesBackend {
    fn new(api_key: String, model: String, timeout: Duration) -> Result<Self, reqwest::Error> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {api_key}")).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let http = Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .build()?;
        Ok(Self { http, model })
    }
}

#[async_trait]
impl Backend for OpenAiResponsesBackend {
    async fn respond(&self, input: InferenceInput) -> Result<NormalizedModelOutput, InferenceError> {
        let request_body = json!({
            "model": self.model,
            "input": [
                {
                    "role": "developer",
                    "content": [
                        {
                            "type": "input_text",
                            "text": developer_instructions(&input.profile)
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": render_user_input(&input)
                        }
                    ]
                }
            ],
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "minilab_agent_runtime_output",
                    "strict": true,
                    "schema": output_schema()
                }
            }
        });

        let response = self.http.post(OPENAI_RESPONSES_URL).json(&request_body).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(InferenceError::Upstream {
                status: status.as_u16(),
                body,
            });
        }
        let value: Value = serde_json::from_str(&body)?;
        let text = extract_output_text(&value).ok_or(InferenceError::MissingOutputText)?;
        let output: NormalizedModelOutput = serde_json::from_str(&text)?;
        Ok(output)
    }
}

fn developer_instructions(profile: &PlaceProfile) -> String {
    format!(
        "You are the inference layer for a Minilab premium-assisted Place. \
Return JSON only. Never claim to have executed a material action. \
You may only produce one of these output classes: advisory, proposal, artifact, handoff_result. \
If the user asks for a material action, command execution, mutation, deployment, sending, pairing, deleting, updating, or anything that changes the world, classify it as proposal and set proposed_action.requires_confirmation=true. \
If the user asks for a report, summary document, file, artifact, or structured deliverable, classify as artifact. \
Otherwise classify as advisory. \
Do not produce execution_summary here. That class is reserved for Minilab-side command/artifact attachment. \
Place profile: place_id={}; label={}; mode={}; allowed_outputs={:?}; forbidden={:?}.",
        profile.place_id, profile.label, profile.mode, profile.allowed_outputs, profile.forbidden
    )
}

fn render_user_input(input: &InferenceInput) -> String {
    let attachments = if input.attachments.is_empty() {
        "none".to_string()
    } else {
        input.attachments
            .iter()
            .map(|file| {
                format!(
                    "{} ({} bytes, {})",
                    file.name,
                    file.size_bytes,
                    file.mime_type.as_deref().unwrap_or("application/octet-stream")
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "User message:\n{}\n\nAttachments: {}\n\nReturn normalized output JSON for the Minilab place runtime.",
        input.text, attachments
    )
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "output_class": {
                "type": "string",
                "enum": ["advisory", "proposal", "artifact", "handoff_result"]
            },
            "acknowledgement": { "type": "string" },
            "reply_text": { "type": "string" },
            "checkpoint_summary": { "type": "string" },
            "policy_state": {
                "type": "string",
                "enum": ["tracked", "approval_required"]
            },
            "proposed_action": {
                "type": ["object", "null"],
                "additionalProperties": false,
                "properties": {
                    "action_kind": { "type": "string" },
                    "invocation_family": { "type": "string" },
                    "requires_confirmation": { "type": "boolean" },
                    "outcome_label": { "type": "string" }
                },
                "required": [
                    "action_kind",
                    "invocation_family",
                    "requires_confirmation",
                    "outcome_label"
                ]
            },
            "artifact": {
                "type": ["object", "null"],
                "additionalProperties": false,
                "properties": {
                    "primary_label": { "type": "string" }
                },
                "required": ["primary_label"]
            }
        },
        "required": [
            "output_class",
            "acknowledgement",
            "reply_text",
            "checkpoint_summary",
            "policy_state",
            "proposed_action",
            "artifact"
        ]
    })
}

fn extract_output_text(value: &Value) -> Option<String> {
    value.get("output")?.as_array()?.iter().find_map(|item| {
        let content = item.get("content")?.as_array()?;
        content.iter().find_map(|content_item| {
            if content_item.get("type").and_then(Value::as_str) == Some("output_text") {
                content_item
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_text_extractor_reads_responses_shape() {
        let value = json!({
            "output": [
                {
                    "type": "message",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "{\"output_class\":\"advisory\",\"acknowledgement\":\"ok\",\"reply_text\":\"hi\",\"checkpoint_summary\":\"done\",\"policy_state\":\"tracked\",\"proposed_action\":null,\"artifact\":null}"
                        }
                    ]
                }
            ]
        });
        let text = extract_output_text(&value).expect("text");
        assert!(text.contains("\"output_class\":\"advisory\""));
    }
}
