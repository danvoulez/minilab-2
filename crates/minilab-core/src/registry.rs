use serde::{Deserialize, Serialize};

use crate::departments::{AutonomyClass, Department};

/// Encodes what a department is allowed to do — the company's operating constitution.
///
/// One `DepartmentSpec` per `Department` variant. Stored in the registry at startup;
/// evaluated at the policy boundary before any department run is admitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDepartmentSpec {
    pub kind: Department,
    pub autonomy_class: AutonomyClass,
    /// IR primitive names this department may emit (subset of `constitutional-runtime` ActionKind).
    pub allowed_primitives: Vec<String>,
    /// High-level action labels (e.g. "score", "send", "classify").
    pub allowed_actions: Vec<String>,
    /// Channel identifiers (e.g. "email", "linkedin").
    pub allowed_channels: Vec<String>,
    /// Actions that require explicit human confirmation before execution.
    pub requires_confirmation_for: Vec<String>,
    pub premium_allowed: bool,
    /// Claims-profile scope identifier, if any content is emitted.
    pub claims_profile_scope: Option<String>,
}

/// When a department run escalates, this rule determines where it goes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRule {
    pub from_department: Department,
    /// Condition expression (evaluated by policy engine; free-form string for now).
    pub condition: String,
    pub to_department: Department,
    pub premium_allowed: bool,
}

/// Per-channel, per-region send policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPolicy {
    /// e.g. "email", "linkedin", "sms"
    pub channel: String,
    /// ISO 3166-1 alpha-2 region code, or "*" for all.
    pub region: String,
    pub allowed: bool,
    /// JSON-serialisable requirements (opt-out check, consent type, etc.).
    pub requirements: serde_json::Value,
    /// JSON-serialisable throttle rules (max/day, min_gap_hours, etc.).
    pub throttle_rules: serde_json::Value,
}

/// The full company registry — loaded once at runtime, never mutated during a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanyRegistry {
    pub departments: Vec<RegistryDepartmentSpec>,
    pub escalation_rules: Vec<EscalationRule>,
    pub channel_policies: Vec<ChannelPolicy>,
}

impl CompanyRegistry {
    pub fn spec_for(&self, kind: Department) -> Option<&RegistryDepartmentSpec> {
        self.departments.iter().find(|s| s.kind == kind)
    }

    pub fn channel_policy(&self, channel: &str, region: &str) -> Option<&ChannelPolicy> {
        self.channel_policies
            .iter()
            .find(|p| p.channel == channel && (p.region == region || p.region == "*"))
    }

    pub fn escalation_rules_from(&self, dept: Department) -> impl Iterator<Item = &EscalationRule> {
        self.escalation_rules
            .iter()
            .filter(move |r| r.from_department == dept)
    }
}
