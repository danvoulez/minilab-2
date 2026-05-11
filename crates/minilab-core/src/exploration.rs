use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::business_canon::DepartmentKind;
use crate::elastic_config::{ElasticOperatingConfig, RuntimeBranch};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationEngine {
    pub baseline: ElasticOperatingConfig,
    pub strategies: Vec<ExplorationStrategy>,
    pub runs: Vec<ExplorationRun>,
}

impl ExplorationEngine {
    pub fn new(baseline: ElasticOperatingConfig) -> Self {
        Self {
            baseline,
            strategies: vec![],
            runs: vec![],
        }
    }

    pub fn branch_from_variations(
        &self,
        id: impl Into<String>,
        spec: SimulationSpec,
        apply: impl FnOnce(&mut ElasticOperatingConfig),
    ) -> ExplorationRun {
        let mut modified = self.baseline.clone();
        apply(&mut modified);

        ExplorationRun {
            id: id.into(),
            branch_config: RuntimeBranch {
                base: self.baseline.clone(),
                modified,
                spec: spec.clone(),
            },
            spec,
            results: SimulationResult::empty(),
            classification: RegimeClassification::Unstable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExplorationStrategy {
    LocalSearch,
    StressSweep,
    RegimeSweep,
    StrategyGrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationRun {
    pub id: String,
    pub spec: SimulationSpec,
    pub branch_config: RuntimeBranch,
    pub results: SimulationResult,
    pub classification: RegimeClassification,
}

impl ExplorationRun {
    pub fn with_result(mut self, results: SimulationResult) -> Self {
        self.classification = classify_regime(&results);
        self.results = results;
        self
    }

    pub fn merge_proposal(&self) -> Option<MergeDecision> {
        strongest_merge_decision(&self.results.findings)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSpec {
    pub kind: SimulationKind,
    pub target: SimulationTarget,
    pub baseline_ref: String,
    pub allowed_variations: Vec<Variation>,
    pub constraints: Vec<Constraint>,
    pub metrics: Vec<MetricSpecRef>,
    pub merge_policy: MergePolicy,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SimulationKind {
    Stress,
    StrategyTest,
    RegimeTest,
    Replay,
    Counterfactual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimulationTarget {
    Company,
    Department(DepartmentKind),
    Campaign(String),
    ClaimsProfile(String),
    Ingress,
    ResourceAllocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variation {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSpecRef {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergePolicy {
    NoMerge,
    AdvisoryOnly,
    ProtectiveAutoRestrict,
    CanaryEligible,
    RatificationRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub metrics: BTreeMap<String, f64>,
    pub findings: Vec<SimulationFinding>,
    pub notes: Vec<String>,
}

impl SimulationResult {
    pub fn empty() -> Self {
        Self {
            metrics: BTreeMap::new(),
            findings: vec![],
            notes: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationFinding {
    pub id: String,
    pub kind: FindingKind,
    pub summary: String,
    pub evidence_ref: Option<String>,
    pub recommended_merge: Option<MergeDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FindingKind {
    Critical,
    Unsafe,
    Dominance,
    TradeOff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegimeClassification {
    Dominant,
    TradeOff,
    Fragile,
    Unstable,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MergeDecision {
    Reject,
    Advisory,
    Canary,
    AdoptElastic,
    RestrictImmediate,
    ProposeBusinessCanonChange,
}

pub fn classify_regime(result: &SimulationResult) -> RegimeClassification {
    let has_critical = result
        .findings
        .iter()
        .any(|f| matches!(f.kind, FindingKind::Critical));
    let has_unsafe = result
        .findings
        .iter()
        .any(|f| matches!(f.kind, FindingKind::Unsafe));
    let has_dominance = result
        .findings
        .iter()
        .any(|f| matches!(f.kind, FindingKind::Dominance));
    let has_tradeoff = result
        .findings
        .iter()
        .any(|f| matches!(f.kind, FindingKind::TradeOff));

    if has_critical {
        RegimeClassification::Invalid
    } else if has_unsafe {
        RegimeClassification::Fragile
    } else if has_dominance && !has_tradeoff {
        RegimeClassification::Dominant
    } else if has_tradeoff {
        RegimeClassification::TradeOff
    } else {
        RegimeClassification::Unstable
    }
}

fn strongest_merge_decision(findings: &[SimulationFinding]) -> Option<MergeDecision> {
    let mut best: Option<MergeDecision> = None;

    for finding in findings {
        if let Some(candidate) = &finding.recommended_merge {
            if rank_merge(candidate) > best.as_ref().map(rank_merge).unwrap_or(0) {
                best = Some(candidate.clone());
            }
        }
    }

    best
}

fn rank_merge(decision: &MergeDecision) -> u8 {
    match decision {
        MergeDecision::Reject => 0,
        MergeDecision::Advisory => 1,
        MergeDecision::Canary => 2,
        MergeDecision::AdoptElastic => 3,
        MergeDecision::RestrictImmediate => 4,
        MergeDecision::ProposeBusinessCanonChange => 5,
    }
}

pub fn stress_spec(
    baseline_ref: impl Into<String>,
    target: SimulationTarget,
    variations: Vec<Variation>,
    seed: u64,
) -> SimulationSpec {
    SimulationSpec {
        kind: SimulationKind::Stress,
        target,
        baseline_ref: baseline_ref.into(),
        allowed_variations: variations,
        constraints: vec![],
        metrics: vec![
            MetricSpecRef {
                name: "positive_reply_rate".to_string(),
            },
            MetricSpecRef {
                name: "premium_exception_rate".to_string(),
            },
            MetricSpecRef {
                name: "evidence_closure_rate".to_string(),
            },
        ],
        merge_policy: MergePolicy::ProtectiveAutoRestrict,
        seed: Some(seed),
    }
}

pub fn strategy_test_spec(
    baseline_ref: impl Into<String>,
    target: SimulationTarget,
    variations: Vec<Variation>,
    seed: Option<u64>,
) -> SimulationSpec {
    SimulationSpec {
        kind: SimulationKind::StrategyTest,
        target,
        baseline_ref: baseline_ref.into(),
        allowed_variations: variations,
        constraints: vec![],
        metrics: vec![
            MetricSpecRef {
                name: "positive_reply_rate".to_string(),
            },
            MetricSpecRef {
                name: "meeting_rate".to_string(),
            },
            MetricSpecRef {
                name: "premium_exception_rate".to_string(),
            },
        ],
        merge_policy: MergePolicy::CanaryEligible,
        seed,
    }
}

pub fn regime_test_spec(
    baseline_ref: impl Into<String>,
    target: SimulationTarget,
    variations: Vec<Variation>,
) -> SimulationSpec {
    SimulationSpec {
        kind: SimulationKind::RegimeTest,
        target,
        baseline_ref: baseline_ref.into(),
        allowed_variations: variations,
        constraints: vec![],
        metrics: vec![
            MetricSpecRef {
                name: "policy_block_rate".to_string(),
            },
            MetricSpecRef {
                name: "autonomy_rate".to_string(),
            },
            MetricSpecRef {
                name: "evidence_closure_rate".to_string(),
            },
        ],
        merge_policy: MergePolicy::RatificationRequired,
        seed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business_canon::minilab_business_canon;
    use crate::elastic_config::default_elastic_config;

    #[test]
    fn classify_critical_as_invalid() {
        let result = SimulationResult {
            metrics: BTreeMap::new(),
            findings: vec![SimulationFinding {
                id: "f1".to_string(),
                kind: FindingKind::Critical,
                summary: "unsubscribe path broke under stress".to_string(),
                evidence_ref: None,
                recommended_merge: Some(MergeDecision::RestrictImmediate),
            }],
            notes: vec![],
        };

        assert_eq!(classify_regime(&result), RegimeClassification::Invalid);
    }

    #[test]
    fn branch_from_variations_uses_modified_config() {
        let canon = minilab_business_canon();
        let baseline = default_elastic_config(&canon);
        let engine = ExplorationEngine::new(baseline.clone());

        let spec = strategy_test_spec(
            baseline.id.clone(),
            SimulationTarget::Company,
            vec![Variation {
                key: "offer.name".to_string(),
                value: serde_json::Value::String("New Offer".to_string()),
            }],
            Some(7),
        );

        let run = engine.branch_from_variations("run-1", spec, |cfg| {
            cfg.offer.name = "New Offer".to_string();
        });

        assert_eq!(run.branch_config.modified.offer.name, "New Offer");
        assert_eq!(run.branch_config.base.id, baseline.id);
    }
}
