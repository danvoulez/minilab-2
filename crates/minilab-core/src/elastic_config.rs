use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::business_canon::{BusinessCanon, DepartmentKind};
use crate::departments::AutonomyClass;
use crate::exploration::SimulationSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElasticOperatingConfig {
    pub id: String,
    pub business_canon_id: String,
    pub active_project: String,
    pub icp: IcpSpec,
    pub offer: OfferSpec,
    pub department_instances: Vec<DepartmentInstance>,
    pub campaigns: Vec<CampaignSpec>,
    pub autonomy_map: BTreeMap<DepartmentKind, AutonomyClass>,
    pub claims_profile_id: String,
    pub resources: ResourceAllocation,
    pub ingress_config: IngressConfig,
    pub version: String,
}

impl ElasticOperatingConfig {
    pub fn department_instance(&self, kind: DepartmentKind) -> Option<&DepartmentInstance> {
        self.department_instances.iter().find(|d| d.kind == kind)
    }

    pub fn active_campaigns(&self) -> impl Iterator<Item = &CampaignSpec> {
        self.campaigns.iter().filter(|c| c.active)
    }

    pub fn validate_against_canon(&self, canon: &BusinessCanon) -> Result<(), ElasticConfigError> {
        if self.business_canon_id != canon.id {
            return Err(ElasticConfigError::BusinessCanonMismatch {
                expected: canon.id.clone(),
                found: self.business_canon_id.clone(),
            });
        }

        if canon.claims_profile(&self.claims_profile_id).is_none() {
            return Err(ElasticConfigError::UnknownClaimsProfile {
                claims_profile_id: self.claims_profile_id.clone(),
            });
        }

        for (kind, autonomy) in &self.autonomy_map {
            let spec = canon
                .department(*kind)
                .ok_or(ElasticConfigError::UnknownDepartment { kind: *kind })?;

            if autonomy_exceeds_canon(*autonomy, spec.autonomy_class) {
                return Err(ElasticConfigError::AutonomyExceedsCanon {
                    kind: *kind,
                    allowed: spec.autonomy_class,
                    requested: *autonomy,
                });
            }
        }

        if self.ingress_config.premium_enabled && self.ingress_config.premium_daily_cap == 0 {
            return Err(ElasticConfigError::InvalidIngressConfig(
                "premium_enabled=true requires premium_daily_cap > 0".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcpSpec {
    pub segment: String,
    pub geography: Option<String>,
    pub company_size: Option<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferSpec {
    pub id: String,
    pub name: String,
    pub persona: String,
    pub core_pain: String,
    pub cta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepartmentInstance {
    pub id: String,
    pub kind: DepartmentKind,
    pub shards: u32,
    pub concurrency_limit: u32,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignSpec {
    pub id: String,
    pub channel: Channel,
    pub message_family: String,
    pub cadence: CadenceSpec,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CadenceSpec {
    pub steps: u32,
    pub spacing_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Channel {
    Email,
    LinkedIn,
    LandingPage,
    WhatsApp,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub budget_units: u64,
    pub premium_budget_units: u64,
    pub compute_tier: ComputeTier,
    pub queue_limits: BTreeMap<DepartmentKind, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeTier {
    SingleMini,
    MultiMini,
    Hybrid,
    CloudBurst,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressConfig {
    pub level0_enabled: bool,
    pub level1_enabled: bool,
    pub level2_enabled: bool,
    pub premium_enabled: bool,
    pub premium_daily_cap: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBranch {
    pub base: ElasticOperatingConfig,
    pub modified: ElasticOperatingConfig,
    pub spec: SimulationSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BranchReason {
    Stress,
    StrategyTest,
    RegimeTest,
    Counterfactual,
    Replay,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElasticConfigError {
    BusinessCanonMismatch {
        expected: String,
        found: String,
    },
    UnknownClaimsProfile {
        claims_profile_id: String,
    },
    UnknownDepartment {
        kind: DepartmentKind,
    },
    AutonomyExceedsCanon {
        kind: DepartmentKind,
        allowed: AutonomyClass,
        requested: AutonomyClass,
    },
    InvalidIngressConfig(String),
}

fn autonomy_exceeds_canon(requested: AutonomyClass, allowed: AutonomyClass) -> bool {
    use AutonomyClass::*;

    match allowed {
        ConstitutionalOnly => requested != ConstitutionalOnly,
        Manual => !matches!(requested, Manual),
        BoundedAutonomous => matches!(requested, HighAutonomy),
        HighAutonomy => false,
    }
}

pub fn default_elastic_config(canon: &BusinessCanon) -> ElasticOperatingConfig {
    let autonomy_map = canon
        .departments
        .iter()
        .map(|d| (d.kind, d.autonomy_class))
        .collect::<BTreeMap<_, _>>();

    ElasticOperatingConfig {
        id: "elastic-default".to_string(),
        business_canon_id: canon.id.clone(),
        active_project: "gtm-wedge-1".to_string(),
        icp: IcpSpec {
            segment: "founder_ratification_required".to_string(),
            geography: Some("founder_ratification_required".to_string()),
            company_size: None,
            triggers: vec!["trust_pressure".to_string(), "ops_complexity".to_string()],
        },
        offer: OfferSpec {
            id: "book_intro".to_string(),
            name: "founder_ratification_required".to_string(),
            persona: "founder_ratification_required".to_string(),
            core_pain: "premium cognition is compensating for missing structure".to_string(),
            cta: "book_intro".to_string(),
        },
        department_instances: vec![
            DepartmentInstance {
                id: "research-1".to_string(),
                kind: DepartmentKind::Research,
                shards: 1,
                concurrency_limit: 4,
                active: true,
            },
            DepartmentInstance {
                id: "salesops-1".to_string(),
                kind: DepartmentKind::SalesOps,
                shards: 1,
                concurrency_limit: 4,
                active: true,
            },
            DepartmentInstance {
                id: "marketing-1".to_string(),
                kind: DepartmentKind::Marketing,
                shards: 1,
                concurrency_limit: 2,
                active: true,
            },
            DepartmentInstance {
                id: "sdr-1".to_string(),
                kind: DepartmentKind::Sdr,
                shards: 1,
                concurrency_limit: 8,
                active: true,
            },
            DepartmentInstance {
                id: "compliance-1".to_string(),
                kind: DepartmentKind::Compliance,
                shards: 1,
                concurrency_limit: 2,
                active: true,
            },
            DepartmentInstance {
                id: "memory-1".to_string(),
                kind: DepartmentKind::MemoryAnalytics,
                shards: 1,
                concurrency_limit: 2,
                active: true,
            },
        ],
        campaigns: vec![CampaignSpec {
            id: "campaign-email-1".to_string(),
            channel: Channel::Email,
            message_family: "mf-trust-audit-a".to_string(),
            cadence: CadenceSpec {
                steps: 4,
                spacing_days: 4,
            },
            active: true,
        }],
        autonomy_map,
        claims_profile_id: "default_gtm".to_string(),
        resources: ResourceAllocation {
            budget_units: 1_000,
            premium_budget_units: 25,
            compute_tier: ComputeTier::SingleMini,
            queue_limits: vec![
                (DepartmentKind::Research, 500),
                (DepartmentKind::SalesOps, 500),
                (DepartmentKind::Marketing, 100),
                (DepartmentKind::Sdr, 1_000),
                (DepartmentKind::Compliance, 100),
                (DepartmentKind::MemoryAnalytics, 100),
            ]
            .into_iter()
            .collect(),
        },
        ingress_config: IngressConfig {
            level0_enabled: true,
            level1_enabled: true,
            level2_enabled: true,
            premium_enabled: true,
            premium_daily_cap: 2,
        },
        version: "0.1.0".to_string(),
    }
}
