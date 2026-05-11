use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::simulation::SimMode;

// --- Enums ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStatus {
    Draft,
    Active,
    Paused,
    Killed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequenceState {
    Pending,
    Sent,
    Cancelled,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyCategory {
    Positive,
    Objection,
    Referral,
    Unsubscribe,
    SpamRisk,
    StrategicAccount,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyScope {
    Send,
    Content,
    Region,
    Strategic,
}

// --- Tables ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub domain: String,
    pub icp_segment: String,
    pub score: f64,
    pub stage: String,
    pub strategic_flag: bool,
    pub dedupe_key: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: Uuid,
    pub account_id: Uuid,
    pub email: String,
    pub phone_e164: Option<String>,
    pub role_inferred: Option<String>,
    pub buyer_persona_id: Option<String>,
    pub opt_out: bool,
    pub opt_out_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEvent {
    pub id: Uuid,
    pub account_id: Option<Uuid>,
    pub source: String,
    pub signal_type: String,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimsProfile {
    pub id: Uuid,
    pub name: String,
    pub claim_ids: Vec<String>,
    pub version: i32,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: Uuid,
    pub name: String,
    pub channel: String,
    pub status: CampaignStatus,
    pub claims_profile_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFamily {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub label: String,
    pub template_ref: String,
    pub claims_allowed: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub account_id: Uuid,
    pub contact_id: Uuid,
    pub step_index: i32,
    pub scheduled_at: DateTime<Utc>,
    pub state: SequenceState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub id: Uuid,
    pub account_id: Uuid,
    pub contact_id: Uuid,
    pub correlation_id: Uuid,
    pub provider: String,
    pub channel_message_id: String,
    pub body: String,
    pub category: ReplyCategory,
    pub classifier_ingress_level: i32,
    pub raw_payload: Value,
    pub received_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Business append-only ledger row (mirrors `EvidenceRecord` in `constitutional-runtime`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceLedgerRow {
    pub id: Uuid,
    pub kind: String,
    pub payload: Value,
    pub correlation_id: Uuid,
    pub idempotency_key: Option<String>,
    pub sim_mode: SimMode,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuyingSignal {
    pub id: Uuid,
    pub account_id: Uuid,
    pub kind: String,
    pub strength: f64,
    pub source: String,
    pub evidence_ref: Option<Uuid>,
    pub observed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Requested,
    Scheduled,
    Completed,
    NoShow,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: Uuid,
    pub account_id: Uuid,
    pub contact_id: Uuid,
    pub campaign_id: Option<Uuid>,
    pub status: MeetingStatus,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepartmentRunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentRun {
    pub id: Uuid,
    pub department_kind: String,
    pub input_ref: Option<Uuid>,
    pub output_ref: Option<Uuid>,
    pub status: DepartmentRunStatus,
    pub evidence_ref: Option<Uuid>,
    pub correlation_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: Uuid,
    pub name: String,
    pub scope: PolicyScope,
    pub expression: Value,
    pub priority: i32,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumExceptionEvent {
    pub id: Uuid,
    pub reason_code: String,
    pub justification: String,
    pub budget_unit: String,
    pub cost_estimate: Option<f64>,
    pub ingress_context: Value,
    pub correlation_id: Uuid,
    pub created_at: DateTime<Utc>,
}
