//! Minilab **company-shaped core**: canonical GTM entities, business canon,
//! elastic operating configuration, exploration, evidence kinds, and the week-1 workflow spine.
//!
//! The split is:
//! - hard constitutional semantics in `constitutional-runtime`
//! - fixed business canon in this crate
//! - elastic operating config in this crate
//! - governed exploration in this crate
//!
//! This crate is intentionally **data + structure**. It pairs with the workspace
//! `constitutional-runtime` crate at the application boundary for IR, validation,
//! lowering, and evidence closure.

pub mod business_canon;
pub mod departments;
pub mod elastic_config;
pub mod entities;
pub mod evidence;
pub mod exploration;
pub mod registry;
pub mod simulation;
pub mod week1;
pub mod workflows;

pub use business_canon::{
    minilab_business_canon, AuthorityModel, BusinessCanon, ClaimsProfileSpec, EntityKind,
    MetricSpec, MetricUnit, PremiumPolicy, PremiumReason, PremiumSteadyState, ValueModel,
};
pub use departments::{AutonomyClass, Department};
pub use elastic_config::{
    default_elastic_config, BranchReason, CadenceSpec, CampaignSpec, Channel, ComputeTier,
    DepartmentInstance, ElasticConfigError, ElasticOperatingConfig, IcpSpec, IngressConfig,
    OfferSpec, ResourceAllocation, RuntimeBranch,
};
pub use entities::*;
pub use evidence::EvidenceKind;
pub use exploration::{
    classify_regime, regime_test_spec, strategy_test_spec, stress_spec, Constraint,
    ExplorationEngine, ExplorationRun, ExplorationStrategy, FindingKind, MergeDecision,
    MergePolicy, RegimeClassification, SimulationFinding, SimulationKind, SimulationResult,
    SimulationSpec, SimulationTarget, Variation,
};
pub use registry::{ChannelPolicy, CompanyRegistry, EscalationRule, RegistryDepartmentSpec};
pub use simulation::{SimBranch, SimMode};
pub use week1::Week1Step;
pub use workflows::{
    campaign_workflow_bundle, workflow_runs_from_config, workflow_specs_for_config, WorkflowInput,
    WorkflowKind, WorkflowOutput, WorkflowRun, WorkflowSpec, WorkflowStatus,
};

use constitutional_runtime::{
    ActionKind, CanonicalActionId, DataRef, DurabilityClass, EvidenceRecord, IRPrimitive, IrNode,
    Kind, NodeId, SurfaceRef, TargetRef, Window,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map};

/// Core planning unit derived from the active elastic operating configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPlan {
    pub department: Department,
    pub primitive: String,
    pub target: WorkTarget,
    pub priority: Priority,
    pub inputs: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkTarget {
    Department(Department),
    Campaign(String),
    Account(String),
    Company,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

/// Derive coarse work plans from the current elastic config.
pub fn work_plans_from_config(config: &ElasticOperatingConfig) -> Vec<WorkPlan> {
    let mut plans = Vec::new();

    if config
        .department_instance(Department::Research)
        .is_some_and(|d| d.active)
    {
        plans.push(WorkPlan {
            department: Department::Research,
            primitive: "COLLECT".to_string(),
            target: WorkTarget::Company,
            priority: Priority::Normal,
            inputs: config.icp.triggers.clone(),
            notes: vec!["refresh market signals".to_string()],
        });
    }

    if config
        .department_instance(Department::SalesOps)
        .is_some_and(|d| d.active)
    {
        plans.push(WorkPlan {
            department: Department::SalesOps,
            primitive: "PRIORITIZE".to_string(),
            target: WorkTarget::Company,
            priority: Priority::High,
            inputs: vec![config.icp.segment.clone(), config.offer.persona.clone()],
            notes: vec!["rank accounts against active wedge".to_string()],
        });
    }

    if config
        .department_instance(Department::Marketing)
        .is_some_and(|d| d.active)
    {
        for campaign in config.active_campaigns() {
            plans.push(WorkPlan {
                department: Department::Marketing,
                primitive: "EMIT".to_string(),
                target: WorkTarget::Campaign(campaign.id.clone()),
                priority: Priority::Normal,
                inputs: vec![
                    campaign.message_family.clone(),
                    config.claims_profile_id.clone(),
                ],
                notes: vec!["compile approved outbound assets".to_string()],
            });
        }
    }

    if config
        .department_instance(Department::Sdr)
        .is_some_and(|d| d.active)
    {
        for campaign in config.active_campaigns() {
            plans.push(WorkPlan {
                department: Department::Sdr,
                primitive: "EXECUTE".to_string(),
                target: WorkTarget::Campaign(campaign.id.clone()),
                priority: Priority::High,
                inputs: vec![format!("channel:{:?}", campaign.channel)],
                notes: vec!["run due outreach steps".to_string()],
            });
        }
    }

    if config
        .department_instance(Department::MemoryAnalytics)
        .is_some_and(|d| d.active)
    {
        plans.push(WorkPlan {
            department: Department::MemoryAnalytics,
            primitive: "COMPARE".to_string(),
            target: WorkTarget::Company,
            priority: Priority::Normal,
            inputs: vec!["current_vs_baseline".to_string()],
            notes: vec!["produce operating deltas".to_string()],
        });
    }

    plans
}

/// Lower a coarse work plan into one constitutional IR node.
pub fn work_plan_to_ir_node(id: impl Into<String>, plan: &WorkPlan) -> IrNode {
    let node_id = id.into();
    let body = match plan.department {
        Department::Research => IRPrimitive::Collect {
            kind: Kind("market_signal".into()),
            target: TargetRef("minilab.market".into()),
            window: Window("7d".into()),
        },
        Department::SalesOps => IRPrimitive::Prioritize {
            kind: Kind("account_queue".into()),
            input_ref: DataRef(
                plan.inputs
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "elastic.icp".to_string()),
            ),
            policy: constitutional_runtime::PolicyId("policy/active_wedge".into()),
            infer_surface: constitutional_runtime::InferSurface::Local,
        },
        Department::Marketing => IRPrimitive::Emit {
            surface: SurfaceRef("minilab.marketing".into()),
            payload: DataRef(
                plan.inputs
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "campaign/message_family".to_string()),
            ),
        },
        Department::Sdr => IRPrimitive::Execute {
            action: ActionKind::Canonical(
                CanonicalActionId::new("outreach", "execute").expect("canonical literal"),
            ),
            params: Map::new(),
        },
        Department::MemoryAnalytics => IRPrimitive::Compare {
            kind: Kind("operating_regime".into()),
            left: DataRef("elastic/current".into()),
            right: DataRef("elastic/baseline".into()),
        },
        _ => IRPrimitive::Persist {
            data: DataRef("minilab/core_state".into()),
            durability: DurabilityClass::Audited,
        },
    };

    IrNode {
        id: NodeId(node_id),
        body,
    }
}

/// Materialize one exploration run into a canonical evidence record.
pub fn evidence_from_exploration_run(run: &ExplorationRun) -> EvidenceRecord {
    EvidenceRecord {
        kind: "simulation.run".to_string(),
        payload_json: json!({
            "run_id": run.id,
            "kind": run.spec.kind,
            "target": run.spec.target,
            "baseline_ref": run.spec.baseline_ref,
            "classification": run.classification,
            "merge_candidate": run.merge_proposal(),
            "metrics": run.results.metrics,
            "findings": run.results.findings,
            "seed": run.spec.seed,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn account_json_roundtrip() {
        let now = Utc::now();
        let a = Account {
            id: Uuid::new_v4(),
            name: "Co".into(),
            domain: "co.example".into(),
            icp_segment: "icp1".into(),
            score: 1.0,
            stage: "new".into(),
            strategic_flag: false,
            dedupe_key: "k".into(),
            metadata: json!({}),
            created_at: now,
            updated_at: now,
        };
        let s = serde_json::to_string(&a).unwrap();
        let b: Account = serde_json::from_str(&s).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.dedupe_key, b.dedupe_key);
    }

    #[test]
    fn week1_order_len() {
        assert_eq!(Week1Step::ORDER.len(), 7);
    }

    #[test]
    fn default_elastic_config_validates_against_canon() {
        let canon = minilab_business_canon();
        let config = default_elastic_config(&canon);
        assert!(config.validate_against_canon(&canon).is_ok());
    }

    #[test]
    fn exploration_run_becomes_evidence() {
        let canon = minilab_business_canon();
        let baseline = default_elastic_config(&canon);
        let engine = ExplorationEngine::new(baseline.clone());
        let spec = strategy_test_spec(
            baseline.id.clone(),
            SimulationTarget::Company,
            vec![],
            Some(7),
        );
        let run = engine
            .branch_from_variations("run-1", spec, |cfg| {
                cfg.offer.name = "New Offer".to_string();
            })
            .with_result(SimulationResult::empty());
        let evidence = evidence_from_exploration_run(&run);
        assert_eq!(evidence.kind, "simulation.run");
        assert_eq!(evidence.payload_json["run_id"], json!("run-1"));
    }
}
