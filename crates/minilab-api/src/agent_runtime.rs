use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use constitutional_runtime::{
    compile_strong_json_to_ir_graph, execute_compiled_plan, plan_ir_graph, AdmissibilityContext,
    CapabilityManifest, DispatchOutcome, Dispatcher, MinilabRuntimeLowerer, NodeId,
    OperationalCommand, OperationalProgram, PrimitiveName, RuntimeTarget,
};

use crate::{app::AppState, error::ApiError};

const CHATGPT_WORKSPACE_PROFILE_JSON: &str =
    include_str!("../place-policy-pack/chatgpt_workspace.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceProfile {
    pub place_id: String,
    pub slug: String,
    pub label: String,
    pub mode: String,
    pub execution_mode: String,
    pub execution_substrate: String,
    pub default_infer_surface: String,
    pub default_emit_surface: String,
    pub fallback_strategy: String,
    pub automatic_fallback: bool,
    pub initial_session_status: String,
    pub allowed_outputs: Vec<String>,
    pub allowed_task_classes: Vec<String>,
    pub denied_task_classes: Vec<String>,
    pub requires_confirmation_for: Vec<String>,
    pub forbidden: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeAction {
    pub action_kind: String,
    pub target_place: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invocation_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEffectivePolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_substrate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_session_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeCheckpoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeArtifactSummary {
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeAuditEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeTerminalSession {
    pub id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connectable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeAuditTrail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_event: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_events: Vec<AgentRuntimeAuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeExternalTool {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substrate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPipelineEvidence {
    pub kind: String,
    pub correlation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimePipelineSummary {
    pub correlation_id: String,
    pub candidate_kind: String,
    pub ir_node_count: usize,
    pub planned_node_count: usize,
    pub dispatched_node_count: usize,
    pub fully_succeeded: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<AgentPipelineEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeSessionSnapshot {
    pub session_id: String,
    pub session_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_or_execution_substrate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_policy: Option<AgentRuntimeEffectivePolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<AgentRuntimeCheckpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_session: Option<AgentRuntimeTerminalSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<AgentRuntimeAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<AgentRuntimeArtifactSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_trail: Option<AgentRuntimeAuditTrail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_tool: Option<AgentRuntimeExternalTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_pipeline: Option<AgentRuntimePipelineSummary>,
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRuntimeSendAck {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub run_id: String,
    pub output_kind: String,
    pub status: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_policy: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optimistic: Option<OptimisticSendState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptimisticSendState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_policy: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct AgentRuntimeSendRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub policy_overrides: Option<serde_json::Value>,
    #[serde(default)]
    pub files: Vec<AgentRuntimeFileRef>,
}

#[derive(Debug, Deserialize)]
pub struct AgentRuntimeFileRef {
    pub name: String,
    #[serde(default)]
    pub size_bytes: usize,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeService {
    inner: Arc<Mutex<AgentRuntimeStore>>,
    profiles: Arc<BTreeMap<String, PlaceProfile>>,
}

#[derive(Debug, Default)]
struct AgentRuntimeStore {
    sessions: BTreeMap<String, AgentRuntimeSessionSnapshot>,
}

impl AgentRuntimeService {
    pub fn new() -> Result<Self, serde_json::Error> {
        let profile: PlaceProfile = serde_json::from_str(CHATGPT_WORKSPACE_PROFILE_JSON)?;
        let mut profiles = BTreeMap::new();
        profiles.insert(profile.place_id.clone(), profile.clone());
        profiles.insert(profile.slug.clone(), profile);
        Ok(Self {
            inner: Arc::new(Mutex::new(AgentRuntimeStore::default())),
            profiles: Arc::new(profiles),
        })
    }

    pub fn get_profile(&self, place_id_or_slug: &str) -> Option<&PlaceProfile> {
        self.profiles.get(place_id_or_slug)
    }

    pub fn submit_message(
        &self,
        place_id_or_slug: &str,
        request: AgentRuntimeSendRequest,
    ) -> Result<AgentRuntimeSendAck, ApiError> {
        let profile = self.require_profile(place_id_or_slug)?;
        let text = request.text.trim();
        if text.is_empty() {
            return Err(ApiError::bad_request("message text cannot be empty"));
        }
        let draft = classify_message(text, profile, &request.files);
        let output_kind = draft.output_kind.clone();
        let ack = self.record_draft(profile, request.session_id, request.app_id, draft)?;
        let pipeline = run_governed_message_pipeline(profile, text, &ack.session_id, &output_kind)?;
        self.attach_runtime_pipeline(&ack.session_id, pipeline)?;
        Ok(ack)
    }

    pub fn submit_place_intent(
        &self,
        place_id_or_slug: &str,
        session_id: Option<String>,
        intent_kind: String,
        payload: Value,
    ) -> Result<AgentRuntimeSendAck, ApiError> {
        let profile = self.require_profile(place_id_or_slug)?;
        let draft = governed_action_draft(
            profile,
            "submit_place_intent",
            intent_kind,
            payload,
            "proposal",
            "Governed place intent captured and waiting for confirmation.",
            "Awaiting operator confirmation for governed place intent.",
            "approval_required",
            "governed_place_intent",
        );
        self.record_draft(profile, session_id, None, draft)
    }

    pub fn start_governed_handoff(
        &self,
        place_id_or_slug: &str,
        session_id: Option<String>,
        task_kind: String,
        payload: Value,
    ) -> Result<AgentRuntimeSendAck, ApiError> {
        let profile = self.require_profile(place_id_or_slug)?;
        let draft = governed_action_draft(
            profile,
            "start_governed_handoff",
            task_kind,
            payload,
            "handoff_result",
            "Governed handoff requested and queued for review.",
            "Governed handoff opened and waiting on review.",
            "tracked",
            "handoff_requested",
        );
        self.record_draft(profile, session_id, None, draft)
    }

    pub fn submit_operational_action(
        &self,
        place_id_or_slug: &str,
        session_id: Option<String>,
        action_kind: String,
        params: Value,
        policy_context: Option<Value>,
    ) -> Result<AgentRuntimeSendAck, ApiError> {
        let profile = self.require_profile(place_id_or_slug)?;
        let payload = serde_json::json!({
            "params": params,
            "policy_context": policy_context
        });
        let draft = governed_action_draft(
            profile,
            "submit_operational_action",
            action_kind,
            payload,
            "proposal",
            "Operational action captured as governed proposal.",
            "Awaiting confirmation before operational action can enter the runtime.",
            "approval_required",
            "operational_action_proposed",
        );
        self.record_draft(profile, session_id, None, draft)
    }

    pub fn request_confirmation(&self, action_ref: &str) -> Result<Value, ApiError> {
        if action_ref.trim().is_empty() {
            return Err(ApiError::bad_request("action_ref cannot be empty"));
        }
        Ok(serde_json::json!({
            "status": "confirmation_requested",
            "action_ref": action_ref,
            "governance_mode": "place_governed",
            "note": "OpenAI-side confirmations are additional safeguards only; Minilab-side confirmation remains authoritative."
        }))
    }

    pub fn continue_terminal_session(
        &self,
        terminal_session_id: &str,
        command: &str,
    ) -> Result<Value, ApiError> {
        if terminal_session_id.trim().is_empty() {
            return Err(ApiError::bad_request("terminal session id cannot be empty"));
        }
        if command.trim().is_empty() {
            return Err(ApiError::bad_request("command cannot be empty"));
        }

        let sessions = self.list_sessions()?;
        let maybe_terminal = sessions
            .into_iter()
            .find(|session| session.terminal_session_id.as_deref() == Some(terminal_session_id));

        if let Some(session) = maybe_terminal {
            return Ok(serde_json::json!({
                "status": "confirmation_required",
                "terminal_session_id": terminal_session_id,
                "session_id": session.session_id,
                "place_id": session.place_id,
                "command": command,
                "note": "Terminal continuation is governed and requires explicit confirmation before execution."
            }));
        }

        Ok(serde_json::json!({
            "status": "unavailable",
            "terminal_session_id": terminal_session_id,
            "reason": "terminal_sync_not_implemented"
        }))
    }

    pub fn create_artifact(
        &self,
        session_id: Option<&str>,
        run_id: Option<&str>,
        artifact_kind: &str,
        title: &str,
        mime_type: &str,
        content_ref: Option<String>,
        inline_content: Option<String>,
        source_place: &str,
        source_system: &str,
        metadata: Option<Value>,
    ) -> Result<Value, ApiError> {
        if artifact_kind.trim().is_empty() {
            return Err(ApiError::bad_request("artifact_kind cannot be empty"));
        }
        if title.trim().is_empty() {
            return Err(ApiError::bad_request("title cannot be empty"));
        }
        if mime_type.trim().is_empty() {
            return Err(ApiError::bad_request("mime_type cannot be empty"));
        }

        let artifact_id = Uuid::new_v4().to_string();
        let label = format!("{artifact_kind}:{title}");
        let note = format!("Artifact attached from {source_system}: {title}");

        self.update_snapshot(session_id, run_id, |snapshot| {
            bump_artifact_summary(snapshot, &label);
            push_audit_event(snapshot, "artifact.created", &note);
            snapshot.checkpoint = Some(AgentRuntimeCheckpoint {
                phase: Some("completed".into()),
                summary: Some("External artifact attached to the run.".into()),
            });
        })?;

        Ok(serde_json::json!({
            "status": "artifact_created",
            "artifact_id": artifact_id,
            "artifact_kind": artifact_kind,
            "title": title,
            "mime_type": mime_type,
            "content_ref": content_ref,
            "inline_content": inline_content,
            "source_place": source_place,
            "source_system": source_system,
            "metadata": metadata
        }))
    }

    pub fn attach_chatgpt_summary(
        &self,
        run_id: &str,
        summary: &str,
        artifact_refs: Vec<String>,
    ) -> Result<Value, ApiError> {
        if summary.trim().is_empty() {
            return Err(ApiError::bad_request("summary cannot be empty"));
        }

        self.update_snapshot(None, Some(run_id), |snapshot| {
            snapshot.reply_text = Some(summary.trim().into());
            snapshot.checkpoint = Some(AgentRuntimeCheckpoint {
                phase: Some("completed".into()),
                summary: Some("external_summary_attached".into()),
            });
            push_audit_event(
                snapshot,
                "external.summary.attached",
                "ChatGPT summary attached to the run.",
            );
            if !artifact_refs.is_empty() {
                bump_artifact_summary(snapshot, &artifact_refs[0]);
            }
        })?;

        Ok(serde_json::json!({
            "status": "summary_attached",
            "run_id": run_id,
            "summary": summary,
            "artifact_refs": artifact_refs,
            "checkpoint_kind": "external_summary_attached"
        }))
    }

    pub fn attach_run_output(
        &self,
        run_id: &str,
        output_class: &str,
        payload: Value,
        summary: &str,
        artifact_refs: Vec<String>,
    ) -> Result<Value, ApiError> {
        validate_output_class(output_class)?;
        if summary.trim().is_empty() {
            return Err(ApiError::bad_request("summary cannot be empty"));
        }

        self.update_snapshot(None, Some(run_id), |snapshot| {
            snapshot.output_kind = Some(output_class.into());
            snapshot.reply_text = Some(summary.trim().into());
            snapshot.session_status = "completed".into();
            snapshot.run_status = Some("completed".into());
            snapshot.phase = Some("completed".into());
            snapshot.pending = false;
            snapshot.checkpoint = Some(AgentRuntimeCheckpoint {
                phase: Some("completed".into()),
                summary: Some(format!("run_output_attached:{output_class}")),
            });
            if let Some(action) = snapshot.action.as_mut() {
                action.status = "completed".into();
                action.outcome_label = Some(format!("{output_class}_attached"));
            }
            for artifact_ref in &artifact_refs {
                bump_artifact_summary(snapshot, artifact_ref);
            }
            push_audit_event(
                snapshot,
                "run.output.attached",
                &format!("Run output attached as `{output_class}`."),
            );
        })?;

        Ok(serde_json::json!({
            "status": "run_output_attached",
            "run_id": run_id,
            "output_class": output_class,
            "payload": payload,
            "summary": summary,
            "artifact_refs": artifact_refs
        }))
    }

    pub fn store_external_result(
        &self,
        run_id: &str,
        result_kind: &str,
        content: Value,
        summary: &str,
        source_place: &str,
        normalization_class: &str,
    ) -> Result<Value, ApiError> {
        validate_output_class(normalization_class)?;
        if result_kind.trim().is_empty() {
            return Err(ApiError::bad_request("result_kind cannot be empty"));
        }
        if summary.trim().is_empty() {
            return Err(ApiError::bad_request("summary cannot be empty"));
        }

        self.update_snapshot(None, Some(run_id), |snapshot| {
            snapshot.reply_text = Some(summary.trim().into());
            snapshot.output_kind = Some(normalization_class.into());
            push_audit_event(
                snapshot,
                "external.result.stored",
                &format!("External result stored from `{source_place}` as `{result_kind}`."),
            );
        })?;

        Ok(serde_json::json!({
            "status": "external_result_stored",
            "run_id": run_id,
            "result_kind": result_kind,
            "content": content,
            "summary": summary,
            "source_place": source_place,
            "normalization_class": normalization_class
        }))
    }

    pub fn append_evidence_note(
        &self,
        session_id: Option<&str>,
        run_id: Option<&str>,
        note: &str,
        kind: Option<&str>,
    ) -> Result<Value, ApiError> {
        if note.trim().is_empty() {
            return Err(ApiError::bad_request("note cannot be empty"));
        }
        let kind = kind.unwrap_or("evidence.note");
        self.update_snapshot(session_id, run_id, |snapshot| {
            push_audit_event(snapshot, kind, note.trim());
        })?;
        Ok(serde_json::json!({
            "status": "evidence_note_appended",
            "session_id": session_id,
            "run_id": run_id,
            "kind": kind,
            "note": note
        }))
    }

    fn require_profile(&self, place_id_or_slug: &str) -> Result<&PlaceProfile, ApiError> {
        self.get_profile(place_id_or_slug)
            .ok_or_else(|| ApiError::bad_request(format!("unknown place `{place_id_or_slug}`")))
    }

    fn update_snapshot<F>(
        &self,
        session_id: Option<&str>,
        run_id: Option<&str>,
        mut update: F,
    ) -> Result<AgentRuntimeSessionSnapshot, ApiError>
    where
        F: FnMut(&mut AgentRuntimeSessionSnapshot),
    {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::upstream("agent runtime store lock poisoned"))?;

        let key = if let Some(session_id) = session_id {
            if !guard.sessions.contains_key(session_id) {
                return Err(ApiError::bad_request(format!(
                    "unknown session `{session_id}`"
                )));
            }
            session_id.to_string()
        } else if let Some(run_id) = run_id {
            guard
                .sessions
                .iter()
                .find(|(_, snapshot)| snapshot.run_id.as_deref() == Some(run_id))
                .map(|(session_id, _)| session_id.clone())
                .ok_or_else(|| ApiError::bad_request(format!("unknown run `{run_id}`")))?
        } else {
            return Err(ApiError::bad_request(
                "either session_id or run_id is required",
            ));
        };

        let snapshot = guard
            .sessions
            .get_mut(&key)
            .expect("validated session key should exist");
        update(snapshot);
        Ok(snapshot.clone())
    }

    fn record_draft(
        &self,
        profile: &PlaceProfile,
        session_id: Option<String>,
        app_id: Option<String>,
        draft: DraftOutcome,
    ) -> Result<AgentRuntimeSendAck, ApiError> {
        let session_id = session_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let run_id = Uuid::new_v4().to_string();

        let snapshot = AgentRuntimeSessionSnapshot {
            session_id: session_id.clone(),
            session_status: draft.session_status.clone(),
            run_id: Some(run_id.clone()),
            app_id: app_id.clone(),
            run_status: Some(draft.run_status.clone()),
            phase: Some(draft.phase.clone()),
            place_id: Some(profile.place_id.clone()),
            provider_or_execution_substrate: Some(profile.execution_substrate.clone()),
            effective_policy: Some(policy_from_profile(profile)),
            checkpoint: Some(AgentRuntimeCheckpoint {
                phase: Some(draft.phase.clone()),
                summary: Some(draft.checkpoint_summary.clone()),
            }),
            terminal_session_id: None,
            terminal_session: None,
            output_kind: Some(draft.output_kind.clone()),
            reply_text: draft.reply_text.clone(),
            action: draft.action.clone(),
            artifacts: draft.artifacts.clone(),
            audit_trail: Some(AgentRuntimeAuditTrail {
                initiated_by: Some("operator".into()),
                execution_surface: Some(profile.execution_substrate.clone()),
                policy_state: Some(draft.policy_state.clone()),
                latest_event: Some(draft.audit_summary.clone()),
                recent_events: vec![
                    AgentRuntimeAuditEvent {
                        kind: Some("session.opened".into()),
                        summary: "Agent session opened or resumed.".into(),
                    },
                    AgentRuntimeAuditEvent {
                        kind: Some("run.created".into()),
                        summary: "Agent run created for inbound message.".into(),
                    },
                    AgentRuntimeAuditEvent {
                        kind: Some("output.normalized".into()),
                        summary: draft.audit_summary.clone(),
                    },
                ],
            }),
            external_tool: Some(AgentRuntimeExternalTool {
                label: profile.label.clone(),
                substrate: Some(profile.execution_substrate.clone()),
                supervision: Some("place_mediated".into()),
            }),
            runtime_pipeline: None,
            pending: false,
        };

        let mut guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::upstream("agent runtime store lock poisoned"))?;
        guard.sessions.insert(session_id.clone(), snapshot.clone());
        drop(guard);

        Ok(AgentRuntimeSendAck {
            session_id,
            app_id,
            run_id,
            output_kind: draft.output_kind,
            status: snapshot.session_status.clone(),
            text: draft.acknowledgement,
            action: snapshot
                .action
                .clone()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| ApiError::upstream(format!("failed to encode action: {err}")))?,
            effective_policy: snapshot
                .effective_policy
                .clone()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| ApiError::upstream(format!("failed to encode policy: {err}")))?,
            optimistic: Some(OptimisticSendState {
                phase: snapshot.phase.clone(),
                checkpoint: snapshot
                    .checkpoint
                    .clone()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|err| {
                        ApiError::upstream(format!("failed to encode checkpoint: {err}"))
                    })?,
                effective_policy: snapshot
                    .effective_policy
                    .clone()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|err| ApiError::upstream(format!("failed to encode policy: {err}")))?,
            }),
        })
    }

    fn attach_runtime_pipeline(
        &self,
        session_id: &str,
        pipeline: AgentRuntimePipelineSummary,
    ) -> Result<(), ApiError> {
        self.update_snapshot(Some(session_id), None, |snapshot| {
            if let Some(audit) = snapshot.audit_trail.as_mut() {
                audit.latest_event = Some(format!(
                    "Agent pipeline closed with {} planned nodes and {} dispatched nodes.",
                    pipeline.planned_node_count, pipeline.dispatched_node_count
                ));
                audit
                    .recent_events
                    .extend(
                        pipeline
                            .evidence
                            .iter()
                            .map(|event| AgentRuntimeAuditEvent {
                                kind: Some(event.kind.clone()),
                                summary: event.summary.clone(),
                            }),
                    );
            }
            snapshot.runtime_pipeline = Some(pipeline);
        })?;
        Ok(())
    }

    pub fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<AgentRuntimeSessionSnapshot>, ApiError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::upstream("agent runtime store lock poisoned"))?;
        Ok(guard.sessions.get(session_id).cloned())
    }

    pub fn resolve_profile(
        &self,
        place_id_or_slug: &str,
    ) -> Result<Option<PlaceProfile>, ApiError> {
        Ok(self.get_profile(place_id_or_slug).cloned())
    }

    pub fn list_sessions(&self) -> Result<Vec<AgentRuntimeSessionSnapshot>, ApiError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::upstream("agent runtime store lock poisoned"))?;
        Ok(guard.sessions.values().cloned().collect())
    }
}

#[derive(Debug, Clone)]
struct DraftOutcome {
    output_kind: String,
    session_status: String,
    run_status: String,
    phase: String,
    acknowledgement: String,
    checkpoint_summary: String,
    reply_text: Option<String>,
    action: Option<AgentRuntimeAction>,
    artifacts: Option<AgentRuntimeArtifactSummary>,
    policy_state: String,
    audit_summary: String,
}

fn governed_action_draft(
    profile: &PlaceProfile,
    action_kind: &str,
    target_kind: String,
    payload: Value,
    output_kind: &str,
    acknowledgement: &str,
    checkpoint_summary: &str,
    policy_state: &str,
    outcome_label: &str,
) -> DraftOutcome {
    let payload_summary = summarize_payload(&payload);
    DraftOutcome {
        output_kind: output_kind.into(),
        session_status: "waiting".into(),
        run_status: "completed".into(),
        phase: "awaiting_input".into(),
        acknowledgement: format!("{} {}", profile.label, acknowledgement),
        checkpoint_summary: checkpoint_summary.into(),
        reply_text: Some(payload_summary.clone()),
        action: Some(AgentRuntimeAction {
            action_kind: action_kind.into(),
            target_place: profile.place_id.clone(),
            status: "proposed".into(),
            invocation_family: Some(target_kind),
            governance_mode: Some("place_governed".into()),
            requires_confirmation: Some(true),
            outcome_label: Some(outcome_label.into()),
        }),
        artifacts: None,
        policy_state: policy_state.into(),
        audit_summary: format!("Governed action staged: {action_kind}."),
    }
}

fn classify_message(
    text: &str,
    profile: &PlaceProfile,
    files: &[AgentRuntimeFileRef],
) -> DraftOutcome {
    let lower = text.to_lowercase();
    let asks_for_material_action = [
        "send ",
        "pair ",
        "delete ",
        "update ",
        "deploy ",
        "continue terminal",
        "run command",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    if asks_for_material_action {
        return DraftOutcome {
            output_kind: "proposal".into(),
            session_status: "waiting".into(),
            run_status: "completed".into(),
            phase: "awaiting_input".into(),
            acknowledgement: format!(
                "{} captured a governed proposal. Confirmation is required before any material action runs.",
                profile.label
            ),
            checkpoint_summary: "Awaiting operator confirmation for governed handoff.".into(),
            reply_text: None,
            action: Some(AgentRuntimeAction {
                action_kind: "submit_place_intent".into(),
                target_place: profile.place_id.clone(),
                status: "proposed".into(),
                invocation_family: Some("chatgpt_workspace".into()),
                governance_mode: Some("place_governed".into()),
                requires_confirmation: Some(true),
                outcome_label: Some("confirmation_required".into()),
            }),
            artifacts: None,
            policy_state: "approval_required".into(),
            audit_summary: "Output normalized as governed proposal.".into(),
        };
    }

    if lower.contains("report") || lower.contains("artifact") || !files.is_empty() {
        return DraftOutcome {
            output_kind: "artifact".into(),
            session_status: "completed".into(),
            run_status: "completed".into(),
            phase: "completed".into(),
            acknowledgement: format!(
                "{} captured an artifact-producing turn and linked it to the session shell.",
                profile.label
            ),
            checkpoint_summary: "Artifact-producing turn closed in shell mode.".into(),
            reply_text: Some(
                "Artifact capture is staged through the adapter shell; evidence ingest comes next."
                    .into(),
            ),
            action: Some(AgentRuntimeAction {
                action_kind: "attach_run_output".into(),
                target_place: profile.place_id.clone(),
                status: "completed".into(),
                invocation_family: Some("chatgpt_workspace".into()),
                governance_mode: Some("place_governed".into()),
                requires_confirmation: Some(false),
                outcome_label: Some("artifact_captured".into()),
            }),
            artifacts: Some(AgentRuntimeArtifactSummary {
                count: usize::max(files.len(), 1),
                primary_label: Some("chatgpt_workspace_output".into()),
            }),
            policy_state: "tracked".into(),
            audit_summary: "Output normalized as artifact.".into(),
        };
    }

    DraftOutcome {
        output_kind: "advisory".into(),
        session_status: "completed".into(),
        run_status: "completed".into(),
        phase: "completed".into(),
        acknowledgement: format!(
            "{} recorded an advisory turn through the constitutional adapter shell.",
            profile.label
        ),
        checkpoint_summary: "Advisory turn closed without material execution.".into(),
        reply_text: Some(
            "This is the first shell of the ChatGPT-backed Place. It records advisory turns, governed proposals, and artifact-shaped outputs without bypassing the runtime."
                .into(),
        ),
        action: Some(AgentRuntimeAction {
            action_kind: "advisory.respond".into(),
            target_place: profile.place_id.clone(),
            status: "completed".into(),
            invocation_family: Some("chatgpt_workspace".into()),
            governance_mode: Some("advisory_only".into()),
            requires_confirmation: Some(false),
            outcome_label: Some("advisory_recorded".into()),
        }),
        artifacts: None,
        policy_state: "tracked".into(),
        audit_summary: "Output normalized as advisory.".into(),
    }
}
fn run_governed_message_pipeline(
    profile: &PlaceProfile,
    text: &str,
    session_id: &str,
    output_kind: &str,
) -> Result<AgentRuntimePipelineSummary, ApiError> {
    let correlation_id = format!("agent-message:{session_id}");
    let candidate_kind = classify_candidate_kind(text, output_kind);
    let strong_json = strong_candidate_json(profile, text, output_kind);
    let graph = compile_strong_json_to_ir_graph(&strong_json).map_err(|err| {
        ApiError::bad_request(format!("agent candidate did not compile to IR: {err}"))
    })?;
    let manifests = vec![agent_runtime_manifest(profile)];
    let ctx = AdmissibilityContext::default();
    let plan = plan_ir_graph(
        graph.clone(),
        OperationalProgram::default(),
        &manifests,
        &ctx,
        &MinilabRuntimeLowerer,
    )
    .map_err(|err| ApiError::bad_request(format!("agent candidate was not admissible: {err}")))?;
    let report = execute_compiled_plan(
        &plan,
        &AgentRuntimeScriptedDispatcher {
            correlation_id: correlation_id.clone(),
        },
    );

    let mut evidence = vec![
        AgentPipelineEvidence {
            kind: "agent.message.received".into(),
            correlation_id: correlation_id.clone(),
            node_id: None,
            summary: format!("Message received for {}.", profile.place_id),
        },
        AgentPipelineEvidence {
            kind: "agent.candidate.classified".into(),
            correlation_id: correlation_id.clone(),
            node_id: None,
            summary: format!("Candidate classified as {candidate_kind}."),
        },
        AgentPipelineEvidence {
            kind: "agent.pipeline.admitted".into(),
            correlation_id: correlation_id.clone(),
            node_id: None,
            summary: format!("IR admitted with {} nodes.", graph.len()),
        },
    ];
    evidence.extend(report.results.iter().map(|result| AgentPipelineEvidence {
        kind: if result.dispatched {
            "agent.node.dispatched".into()
        } else {
            "agent.node.skipped".into()
        },
        correlation_id: correlation_id.clone(),
        node_id: Some(result.node_id.0.clone()),
        summary: format!(
            "{}.{} -> {:?}",
            result.command.namespace, result.command.verb, result.outcome
        ),
    }));
    evidence.push(AgentPipelineEvidence {
        kind: "agent.response.emitted".into(),
        correlation_id: correlation_id.clone(),
        node_id: None,
        summary: format!("Response emitted as {output_kind} after IR planning."),
    });

    Ok(AgentRuntimePipelineSummary {
        correlation_id,
        candidate_kind,
        ir_node_count: graph.len(),
        planned_node_count: plan.node_plans.len(),
        dispatched_node_count: report
            .results
            .iter()
            .filter(|result| result.dispatched)
            .count(),
        fully_succeeded: report.fully_succeeded(),
        evidence,
    })
}

fn classify_candidate_kind(text: &str, output_kind: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if output_kind == "proposal" {
        "operational_confirm_candidate".into()
    } else if lower.contains("drift") {
        "strong_drift_review".into()
    } else {
        "strong_system_review".into()
    }
}

fn strong_candidate_json(profile: &PlaceProfile, text: &str, output_kind: &str) -> String {
    if output_kind == "proposal" {
        return serde_json::json!({
            "Confirm": {
                "role": "operator",
                "action": {
                    "action": "agent.propose",
                    "params": {
                        "place_id": profile.place_id.clone(),
                        "message": text
                    }
                }
            }
        })
        .to_string();
    }

    let body = serde_json::json!({
        "target": profile.place_id.clone(),
        "pipeline": ["Collect", "Compress", "Classify", "Prioritize"],
        "on_success": { "Emit": profile.default_emit_surface.clone() }
    });
    if text.to_ascii_lowercase().contains("drift") {
        serde_json::json!({ "DriftReview": body }).to_string()
    } else {
        serde_json::json!({ "SystemReview": body }).to_string()
    }
}

fn agent_runtime_manifest(profile: &PlaceProfile) -> CapabilityManifest {
    CapabilityManifest {
        substrate_id: profile.execution_substrate.clone(),
        substrate_version: "agent-runtime-v0".into(),
        supported_primitives: BTreeSet::from_iter([
            PrimitiveName::Observe,
            PrimitiveName::Collect,
            PrimitiveName::Fetch,
            PrimitiveName::Compress,
            PrimitiveName::Classify,
            PrimitiveName::Prioritize,
            PrimitiveName::Compare,
            PrimitiveName::Route,
            PrimitiveName::Schedule,
            PrimitiveName::Execute,
            PrimitiveName::Emit,
            PrimitiveName::Persist,
            PrimitiveName::Confirm,
            PrimitiveName::Cancel,
            PrimitiveName::Reconcile,
        ]),
        declared_guarantees: BTreeSet::from(["evidence.write".into()]),
        ..Default::default()
    }
}

struct AgentRuntimeScriptedDispatcher {
    correlation_id: String,
}

impl Dispatcher for AgentRuntimeScriptedDispatcher {
    fn dispatch(&self, node_id: &NodeId, command: &OperationalCommand) -> DispatchOutcome {
        let target_runtime = match &command.target_runtime {
            RuntimeTarget::MinilabOperationalGrammar => "operational",
            RuntimeTarget::Mcp => "mcp",
            RuntimeTarget::Shell => "shell",
            RuntimeTarget::Cloud => "cloud",
            RuntimeTarget::Platform => "platform",
            RuntimeTarget::Provider => "provider",
        };
        DispatchOutcome::Success {
            evidence_ref: Some(format!("{}:{}", self.correlation_id, node_id.0)),
            detail: Some(serde_json::json!({
                "target_runtime": target_runtime,
                "namespace": command.namespace.clone(),
                "verb": command.verb.clone()
            })),
        }
    }
}

fn policy_from_profile(profile: &PlaceProfile) -> AgentRuntimeEffectivePolicy {
    AgentRuntimeEffectivePolicy {
        profile_id: Some(profile.place_id.clone()),
        execution_mode: Some(profile.execution_mode.clone()),
        execution_substrate: Some(profile.execution_substrate.clone()),
        fallback_strategy: Some(profile.fallback_strategy.clone()),
        automatic_fallback: Some(profile.automatic_fallback),
        initial_session_status: Some(profile.initial_session_status.clone()),
    }
}

fn summarize_payload(payload: &Value) -> String {
    let text = serde_json::to_string(payload).unwrap_or_else(|_| "{}".into());
    if text.len() > 220 {
        format!("Governed payload staged: {}...", &text[..220])
    } else {
        format!("Governed payload staged: {text}")
    }
}

fn validate_output_class(output_class: &str) -> Result<(), ApiError> {
    match output_class {
        "advisory" | "proposal" | "artifact" | "execution_summary" | "handoff_result" => Ok(()),
        other => Err(ApiError::bad_request(format!(
            "unsupported output_class `{other}`"
        ))),
    }
}

fn bump_artifact_summary(snapshot: &mut AgentRuntimeSessionSnapshot, label: &str) {
    match snapshot.artifacts.as_mut() {
        Some(summary) => {
            summary.count += 1;
            if summary.primary_label.is_none() {
                summary.primary_label = Some(label.into());
            }
        }
        None => {
            snapshot.artifacts = Some(AgentRuntimeArtifactSummary {
                count: 1,
                primary_label: Some(label.into()),
            });
        }
    }
}

fn push_audit_event(snapshot: &mut AgentRuntimeSessionSnapshot, kind: &str, summary: &str) {
    let trail = snapshot
        .audit_trail
        .get_or_insert_with(|| AgentRuntimeAuditTrail {
            initiated_by: Some("operator".into()),
            execution_surface: snapshot.provider_or_execution_substrate.clone(),
            policy_state: Some("tracked".into()),
            latest_event: None,
            recent_events: vec![],
        });
    trail.latest_event = Some(summary.into());
    trail.recent_events.insert(
        0,
        AgentRuntimeAuditEvent {
            kind: Some(kind.into()),
            summary: summary.into(),
        },
    );
    if trail.recent_events.len() > 8 {
        trail.recent_events.truncate(8);
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/places/{place_id}/messages", post(post_message))
        .route("/sessions/{session_id}", get(get_session))
}

async fn post_message(
    State(state): State<AppState>,
    Path(place_id): Path<String>,
    Json(body): Json<AgentRuntimeSendRequest>,
) -> Result<Json<AgentRuntimeSendAck>, ApiError> {
    let ack = state.agent_runtime.submit_message(&place_id, body)?;
    Ok(Json(ack))
}

async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<AgentRuntimeSessionSnapshot>, ApiError> {
    let snapshot = state
        .agent_runtime
        .get_session(&session_id)?
        .ok_or_else(|| ApiError::bad_request(format!("unknown session `{session_id}`")))?;
    Ok(Json(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_profile_loads() {
        let service = AgentRuntimeService::new().expect("embedded profile should parse");
        let profile = service
            .get_profile("chatgpt_workspace")
            .expect("chatgpt_workspace profile should exist");
        assert_eq!(profile.execution_substrate, "chatgpt_business");
        assert!(profile
            .allowed_outputs
            .iter()
            .any(|value| value == "proposal"));
    }

    #[test]
    fn material_request_becomes_governed_proposal() {
        let service = AgentRuntimeService::new().expect("embedded profile should parse");
        let ack = service
            .submit_message(
                "chatgpt_workspace",
                AgentRuntimeSendRequest {
                    session_id: None,
                    text: "send the update to the customer".into(),
                    app_id: None,
                    policy_overrides: None,
                    files: vec![],
                },
            )
            .expect("send should succeed");
        assert_eq!(ack.output_kind, "proposal");
        assert_eq!(ack.status, "waiting");
        let snapshot = service
            .get_session(&ack.session_id)
            .expect("read should succeed")
            .expect("session should exist");
        let pipeline = snapshot
            .runtime_pipeline
            .as_ref()
            .expect("proposal output must still touch IR before response");
        assert_eq!(pipeline.candidate_kind, "operational_confirm_candidate");
        assert!(pipeline
            .evidence
            .iter()
            .any(|event| event.kind == "agent.response.emitted"));
    }

    #[test]
    fn advisory_request_roundtrips_as_snapshot() {
        let service = AgentRuntimeService::new().expect("embedded profile should parse");
        let ack = service
            .submit_message(
                "chatgpt_workspace",
                AgentRuntimeSendRequest {
                    session_id: None,
                    text: "summarize the current state".into(),
                    app_id: Some("chatgpt_workspace_agent".into()),
                    policy_overrides: None,
                    files: vec![],
                },
            )
            .expect("send should succeed");
        let snapshot = service
            .get_session(&ack.session_id)
            .expect("read should succeed")
            .expect("session should exist");
        assert_eq!(snapshot.output_kind.as_deref(), Some("advisory"));
        let pipeline = snapshot
            .runtime_pipeline
            .as_ref()
            .expect("advisory output must pass through the runtime pipeline");
        assert_eq!(pipeline.candidate_kind, "strong_system_review");
        assert!(pipeline.fully_succeeded);
        assert!(pipeline
            .evidence
            .iter()
            .any(|event| event.kind == "agent.pipeline.admitted"));
        assert_eq!(
            snapshot.provider_or_execution_substrate.as_deref(),
            Some("chatgpt_business")
        );
    }

    #[test]
    fn submit_place_intent_records_waiting_snapshot() {
        let service = AgentRuntimeService::new().expect("embedded profile should parse");
        let ack = service
            .submit_place_intent(
                "chatgpt_workspace",
                None,
                "drift_review".into(),
                serde_json::json!({ "target": "runtime", "window": "24h" }),
            )
            .expect("place intent should succeed");
        let snapshot = service
            .get_session(&ack.session_id)
            .expect("read should succeed")
            .expect("session should exist");
        assert_eq!(snapshot.output_kind.as_deref(), Some("proposal"));
        assert_eq!(snapshot.session_status, "waiting");
    }

    #[test]
    fn attach_run_output_updates_snapshot_and_artifacts() {
        let service = AgentRuntimeService::new().expect("embedded profile should parse");
        let ack = service
            .submit_place_intent(
                "chatgpt_workspace",
                None,
                "drift_review".into(),
                serde_json::json!({ "target": "runtime" }),
            )
            .expect("place intent should succeed");
        service
            .attach_run_output(
                &ack.run_id,
                "execution_summary",
                serde_json::json!({ "result": "ok" }),
                "Run finished with external execution summary.",
                vec!["artifact-1".into()],
            )
            .expect("attach_run_output should succeed");
        let snapshot = service
            .get_session(&ack.session_id)
            .expect("read should succeed")
            .expect("session should exist");
        assert_eq!(snapshot.output_kind.as_deref(), Some("execution_summary"));
        assert_eq!(snapshot.artifacts.as_ref().map(|a| a.count), Some(1));
        assert_eq!(snapshot.session_status, "completed");
    }
}
