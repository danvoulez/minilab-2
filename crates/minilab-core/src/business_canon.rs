use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::departments::{AutonomyClass, Department};

pub type DepartmentKind = Department;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessCanon {
    pub id: String,
    pub name: String,
    pub thesis: String,
    pub value_model: ValueModel,
    pub departments: Vec<DepartmentSpec>,
    pub core_metrics: Vec<MetricSpec>,
    pub invariant_policies: Vec<PolicyRule>,
    pub premium_policy: PremiumPolicy,
    pub authority_model: AuthorityModel,
    pub claims_profiles: Vec<ClaimsProfileSpec>,
    pub version: String,
}

impl BusinessCanon {
    pub fn department(&self, kind: DepartmentKind) -> Option<&DepartmentSpec> {
        self.departments.iter().find(|d| d.kind == kind)
    }

    pub fn claims_profile(&self, id: &str) -> Option<&ClaimsProfileSpec> {
        self.claims_profiles.iter().find(|p| p.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueModel {
    Consulting,
    SaaS,
    Hybrid,
    InternalOps,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentSpec {
    pub kind: DepartmentKind,
    pub autonomy_class: AutonomyClass,
    pub owned_entities: Vec<EntityKind>,
    pub allowed_primitives: Vec<String>,
    pub allowed_actions: Vec<String>,
    pub premium_allowed: bool,
    pub requires_confirmation_for: Vec<String>,
}

impl DepartmentSpec {
    pub fn owns(&self, entity: EntityKind) -> bool {
        self.owned_entities.contains(&entity)
    }

    pub fn allows_primitive(&self, primitive: &str) -> bool {
        self.allowed_primitives.iter().any(|p| p == primitive)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    Account,
    Contact,
    TriggerEvent,
    BuyingSignal,
    Campaign,
    MessageFamily,
    Sequence,
    Reply,
    Meeting,
    PolicyRule,
    ClaimsProfile,
    PremiumExceptionEvent,
    DepartmentRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpec {
    pub name: String,
    pub description: String,
    pub unit: MetricUnit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MetricUnit {
    Count,
    Percent,
    Currency,
    DurationMs,
    Score,
    Ratio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: String,
    pub scope: String,
    pub rule_kind: String,
    pub config: BTreeMap<String, serde_json::Value>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumPolicy {
    pub target_steady_state: PremiumSteadyState,
    pub allowed_reasons: Vec<PremiumReason>,
    pub requires_logging: bool,
    pub requires_budget_check: bool,
}

impl PremiumPolicy {
    pub fn allows(&self, reason: &PremiumReason) -> bool {
        self.allowed_reasons.iter().any(|r| r == reason)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PremiumSteadyState {
    NearZero,
    Low,
    Moderate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PremiumReason {
    NoveltyExceeded,
    AmbiguityUnresolved,
    StrategicSynthesis,
    LegalSensitive,
    ConstitutionalConflict,
    HighValueDeadlock,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityModel {
    pub constitutional_authority: Vec<String>,
    pub ratification_required_for: Vec<String>,
}

impl AuthorityModel {
    pub fn requires_ratification(&self, subject: &str) -> bool {
        self.ratification_required_for.iter().any(|s| s == subject)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimsProfileSpec {
    pub id: String,
    pub name: String,
    pub allowed_claims: Vec<String>,
    pub forbidden_claims: Vec<String>,
    pub required_disclaimers: Vec<String>,
}

impl ClaimsProfileSpec {
    pub fn allows_claim(&self, claim: &str) -> bool {
        self.allowed_claims.iter().any(|c| c == claim)
            && !self.forbidden_claims.iter().any(|c| c == claim)
    }
}

pub fn minilab_business_canon() -> BusinessCanon {
    BusinessCanon {
        id: "minilab".to_string(),
        name: "Minilab".to_string(),
        thesis: "company as constitutional runtime".to_string(),
        value_model: ValueModel::Hybrid,
        departments: vec![
            DepartmentSpec {
                kind: Department::Research,
                autonomy_class: AutonomyClass::HighAutonomy,
                owned_entities: vec![EntityKind::TriggerEvent, EntityKind::BuyingSignal],
                allowed_primitives: vec![
                    "OBSERVE".to_string(),
                    "COLLECT".to_string(),
                    "CLASSIFY".to_string(),
                    "PRIORITIZE".to_string(),
                    "COMPARE".to_string(),
                ],
                allowed_actions: vec!["market.refresh".to_string(), "signals.rank".to_string()],
                premium_allowed: false,
                requires_confirmation_for: vec![],
            },
            DepartmentSpec {
                kind: Department::SalesOps,
                autonomy_class: AutonomyClass::HighAutonomy,
                owned_entities: vec![
                    EntityKind::Account,
                    EntityKind::Contact,
                    EntityKind::BuyingSignal,
                ],
                allowed_primitives: vec![
                    "FETCH".to_string(),
                    "CLASSIFY".to_string(),
                    "PRIORITIZE".to_string(),
                    "PERSIST".to_string(),
                ],
                allowed_actions: vec!["account.refresh".to_string(), "graph.rank".to_string()],
                premium_allowed: false,
                requires_confirmation_for: vec![],
            },
            DepartmentSpec {
                kind: Department::Marketing,
                autonomy_class: AutonomyClass::BoundedAutonomous,
                owned_entities: vec![EntityKind::Campaign, EntityKind::MessageFamily],
                allowed_primitives: vec![
                    "DECIDE".to_string(),
                    "EMIT".to_string(),
                    "PERSIST".to_string(),
                    "COMPARE".to_string(),
                ],
                allowed_actions: vec![
                    "campaign.compile".to_string(),
                    "landing.generate".to_string(),
                ],
                premium_allowed: true,
                requires_confirmation_for: vec!["claims.high_risk".to_string()],
            },
            DepartmentSpec {
                kind: Department::Sdr,
                autonomy_class: AutonomyClass::HighAutonomy,
                owned_entities: vec![EntityKind::Sequence, EntityKind::Reply, EntityKind::Meeting],
                allowed_primitives: vec![
                    "EXECUTE".to_string(),
                    "SCHEDULE".to_string(),
                    "CLASSIFY".to_string(),
                    "ROUTE".to_string(),
                    "CANCEL".to_string(),
                ],
                allowed_actions: vec!["outreach.send".to_string(), "reply.route".to_string()],
                premium_allowed: false,
                requires_confirmation_for: vec![],
            },
            DepartmentSpec {
                kind: Department::Compliance,
                autonomy_class: AutonomyClass::ConstitutionalOnly,
                owned_entities: vec![EntityKind::PolicyRule, EntityKind::ClaimsProfile],
                allowed_primitives: vec![
                    "CONFIRM".to_string(),
                    "CANCEL".to_string(),
                    "PERSIST".to_string(),
                ],
                allowed_actions: vec!["policy.block".to_string(), "claims.freeze".to_string()],
                premium_allowed: false,
                requires_confirmation_for: vec![],
            },
            DepartmentSpec {
                kind: Department::MemoryAnalytics,
                autonomy_class: AutonomyClass::BoundedAutonomous,
                owned_entities: vec![EntityKind::DepartmentRun, EntityKind::PremiumExceptionEvent],
                allowed_primitives: vec![
                    "COMPARE".to_string(),
                    "COMPRESS".to_string(),
                    "CLASSIFY".to_string(),
                    "PERSIST".to_string(),
                ],
                allowed_actions: vec!["report.weekly".to_string(), "memory.refresh".to_string()],
                premium_allowed: false,
                requires_confirmation_for: vec![],
            },
            DepartmentSpec {
                kind: Department::Founder,
                autonomy_class: AutonomyClass::ConstitutionalOnly,
                owned_entities: vec![],
                allowed_primitives: vec!["CONFIRM".to_string(), "PERSIST".to_string()],
                allowed_actions: vec!["canon.amend".to_string(), "strategy.ratify".to_string()],
                premium_allowed: true,
                requires_confirmation_for: vec![],
            },
        ],
        core_metrics: vec![
            MetricSpec {
                name: "positive_reply_rate".to_string(),
                description: "Positive replies divided by sends.".to_string(),
                unit: MetricUnit::Percent,
            },
            MetricSpec {
                name: "premium_exception_rate".to_string(),
                description: "Premium exceptions per operating period.".to_string(),
                unit: MetricUnit::Count,
            },
            MetricSpec {
                name: "evidence_closure_rate".to_string(),
                description: "Closed evidence records divided by material actions.".to_string(),
                unit: MetricUnit::Percent,
            },
        ],
        invariant_policies: vec![
            PolicyRule {
                id: "opt_out_hard_stop".to_string(),
                scope: "outreach".to_string(),
                rule_kind: "hard_stop".to_string(),
                config: BTreeMap::new(),
                enabled: true,
            },
            PolicyRule {
                id: "premium_near_zero_steady_state".to_string(),
                scope: "ingress".to_string(),
                rule_kind: "budget_doctrine".to_string(),
                config: BTreeMap::new(),
                enabled: true,
            },
        ],
        premium_policy: PremiumPolicy {
            target_steady_state: PremiumSteadyState::NearZero,
            allowed_reasons: vec![
                PremiumReason::NoveltyExceeded,
                PremiumReason::AmbiguityUnresolved,
                PremiumReason::StrategicSynthesis,
                PremiumReason::LegalSensitive,
                PremiumReason::ConstitutionalConflict,
                PremiumReason::HighValueDeadlock,
            ],
            requires_logging: true,
            requires_budget_check: true,
        },
        authority_model: AuthorityModel {
            constitutional_authority: vec!["founder".to_string(), "compliance".to_string()],
            ratification_required_for: vec![
                "business_canon_change".to_string(),
                "department_power_expansion".to_string(),
                "claims_profile_broadening".to_string(),
                "premium_policy_relaxation".to_string(),
            ],
        },
        claims_profiles: vec![ClaimsProfileSpec {
            id: "default_gtm".to_string(),
            name: "Default GTM Claims".to_string(),
            allowed_claims: vec![
                "auditable".to_string(),
                "bounded_autonomy".to_string(),
                "evidence_closure".to_string(),
                "premium_cost_reduction".to_string(),
            ],
            forbidden_claims: vec![
                "guaranteed_compliance".to_string(),
                "fully_autonomous_legal_substitution".to_string(),
            ],
            required_disclaimers: vec!["subject_to_customer_environment".to_string()],
        }],
        version: "0.1.0".to_string(),
    }
}
