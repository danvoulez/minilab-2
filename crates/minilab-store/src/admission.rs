//! World-facing admission orchestration for constitutional LogLine acts.
//!
//! The constitutional decision remains in
//! [`constitutional_runtime::evaluate_admission`]. This module owns the world
//! boundary: load context, call the pure evaluator, then commit the resulting
//! ruling according to the admission commit rules.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use constitutional_runtime::{
    evaluate_admission, AdmissionContext, AdmissionDecision, AdmissionRuling, ProposedLogLineAct,
};
use serde::{Deserialize, Serialize};

use crate::client::{StoreClient, StoreError};

/// Receipt emitted by a world adapter after an admission ruling is committed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissionCommitReceipt {
    pub decision: AdmissionDecision,
    pub proposed_act_hash: String,
    pub ruling_written: bool,
    pub proposed_act_written: bool,
    pub touches_world: bool,
    pub world: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ruling_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_act_id: Option<String>,
}

/// Port implemented by world adapters that can load admission context and
/// persist the resulting admission ruling.
#[async_trait]
pub trait AdmissionWorld: Send + Sync {
    async fn load_admission_context(
        &self,
        proposed: &ProposedLogLineAct,
    ) -> Result<AdmissionContext, StoreError>;

    async fn commit_admission(
        &self,
        proposed: &ProposedLogLineAct,
        ruling: &AdmissionRuling,
    ) -> Result<AdmissionCommitReceipt, StoreError>;
}

/// Application service that bridges a world adapter to the pure constitutional
/// evaluator. It deliberately contains no policy branch of its own.
#[derive(Clone, Debug)]
pub struct AdmissionService<W> {
    world: W,
}

impl<W: AdmissionWorld> AdmissionService<W> {
    pub fn new(world: W) -> Self {
        Self { world }
    }

    pub async fn admit(
        &self,
        proposed: &ProposedLogLineAct,
    ) -> Result<AdmissionCommitReceipt, StoreError> {
        let context = self.world.load_admission_context(proposed).await?;
        let ruling = evaluate_admission(proposed, &context);
        self.world.commit_admission(proposed, &ruling).await
    }
}

/// Explicitly test-only in-memory adapter. It is useful for exercising the
/// orchestration path, but it is not a production adapter and never claims to
/// persist institutional state.
#[derive(Clone, Debug)]
pub struct InMemoryAdmissionWorld {
    context: AdmissionContext,
    state: Arc<Mutex<InMemoryAdmissionState>>,
    world_name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InMemoryAdmissionState {
    pub rulings: Vec<AdmissionRuling>,
    pub admitted_acts: Vec<ProposedLogLineAct>,
}

impl InMemoryAdmissionWorld {
    pub const WORLD_NAME: &'static str = "in-memory-test-admission-world";

    pub fn new(context: AdmissionContext) -> Self {
        Self {
            context,
            state: Arc::new(Mutex::new(InMemoryAdmissionState::default())),
            world_name: Self::WORLD_NAME.into(),
        }
    }

    pub fn world_name(&self) -> &str {
        &self.world_name
    }

    pub fn is_production_adapter(&self) -> bool {
        false
    }

    pub fn snapshot(&self) -> Result<InMemoryAdmissionState, StoreError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| StoreError::Contract("in-memory admission world lock poisoned".into()))?
            .clone())
    }
}

#[async_trait]
impl AdmissionWorld for InMemoryAdmissionWorld {
    async fn load_admission_context(
        &self,
        _proposed: &ProposedLogLineAct,
    ) -> Result<AdmissionContext, StoreError> {
        Ok(self.context.clone())
    }

    async fn commit_admission(
        &self,
        proposed: &ProposedLogLineAct,
        ruling: &AdmissionRuling,
    ) -> Result<AdmissionCommitReceipt, StoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| StoreError::Contract("in-memory admission world lock poisoned".into()))?;

        let ruling_id = format!("mem-ruling-{}", state.rulings.len() + 1);
        state.rulings.push(ruling.clone());

        let proposed_act_id = if ruling.decision == AdmissionDecision::Yes {
            let mut closed_act = proposed.clone();
            closed_act.status = "closed".into();
            let act_id = format!("mem-proposed-act-{}", state.admitted_acts.len() + 1);
            state.admitted_acts.push(closed_act);
            Some(act_id)
        } else {
            None
        };

        Ok(AdmissionCommitReceipt {
            decision: ruling.decision,
            proposed_act_hash: ruling.proposed_act_hash.clone(),
            ruling_written: true,
            proposed_act_written: proposed_act_id.is_some(),
            touches_world: false,
            world: self.world_name.clone(),
            reasons: ruling.reasons.clone(),
            ruling_id: Some(ruling_id),
            proposed_act_id,
        })
    }
}

/// Supabase adapter placeholder.
///
/// The repository currently has a reusable [`StoreClient`] but no
/// `ops.logline_acts` / `logline_acts` schema to load or append constitutional
/// acts. This struct exists so callers have an explicit integration point; it
/// fails closed until a real LogLine acts table/schema is present.
#[derive(Clone)]
pub struct SupabaseAdmissionWorld {
    #[allow(dead_code)]
    client: StoreClient,
}

impl SupabaseAdmissionWorld {
    pub fn new(client: StoreClient) -> Self {
        Self { client }
    }

    pub fn world_name(&self) -> &'static str {
        "supabase-admission-world-unconfigured"
    }

    fn not_configured() -> StoreError {
        StoreError::Contract(
            "SupabaseAdmissionWorld not configured: no ops.logline_acts/logline_acts schema found"
                .into(),
        )
    }
}

#[async_trait]
impl AdmissionWorld for SupabaseAdmissionWorld {
    async fn load_admission_context(
        &self,
        _proposed: &ProposedLogLineAct,
    ) -> Result<AdmissionContext, StoreError> {
        Err(Self::not_configured())
    }

    async fn commit_admission(
        &self,
        _proposed: &ProposedLogLineAct,
        _ruling: &AdmissionRuling,
    ) -> Result<AdmissionCommitReceipt, StoreError> {
        Err(Self::not_configured())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constitutional_runtime::{Boundary, BoundaryKind, BoundaryRule, Gate, Passport, Visa};
    use serde_json::json;

    fn valid_act() -> ProposedLogLineAct {
        ProposedLogLineAct {
            who: "agent:ana".into(),
            did: "host.inspect".into(),
            this: json!({"target":"lab512"}),
            confirmed_by: json!(["passport:ana"]),
            if_ok: "continue".into(),
            if_doubt: "send to doubt".into(),
            if_not: "reject".into(),
            status: "candidate".into(),
            metadata: None,
        }
    }

    fn valid_context() -> AdmissionContext {
        AdmissionContext {
            passports: vec![Passport {
                passport_id: "passport:ana".into(),
                subject: "agent:ana".into(),
                subject_type: "agent".into(),
                status: "closed".into(),
            }],
            visas: vec![Visa {
                visa_id: "visa:ana".into(),
                holder: "agent:ana".into(),
                allowed_dids: vec!["host.inspect".into()],
                forbidden_dids: vec![],
                valid_from: None,
                valid_until: None,
                status: "closed".into(),
            }],
            gates: vec![Gate {
                gate_id: "gate:host".into(),
                applies_to_dids: Some(vec!["host.inspect".into()]),
                fail_closed: true,
                status: "closed".into(),
            }],
            boundaries: vec![],
            now: None,
        }
    }

    #[tokio::test]
    async fn admission_service_admits_yes_and_commits_ruling_and_act() {
        let world = InMemoryAdmissionWorld::new(valid_context());
        let service = AdmissionService::new(world.clone());

        let receipt = service.admit(&valid_act()).await.unwrap();

        assert_eq!(receipt.decision, AdmissionDecision::Yes);
        assert!(receipt.ruling_written);
        assert!(receipt.proposed_act_written);
        assert_eq!(receipt.world, InMemoryAdmissionWorld::WORLD_NAME);
        let snapshot = world.snapshot().unwrap();
        assert_eq!(snapshot.rulings.len(), 1);
        assert_eq!(snapshot.admitted_acts.len(), 1);
        assert_eq!(snapshot.admitted_acts[0].status, "closed");
    }

    #[tokio::test]
    async fn admission_service_rejects_no_and_commits_only_ruling() {
        let mut context = valid_context();
        context.passports.clear();
        let world = InMemoryAdmissionWorld::new(context);
        let service = AdmissionService::new(world.clone());

        let receipt = service.admit(&valid_act()).await.unwrap();

        assert_eq!(receipt.decision, AdmissionDecision::No);
        assert!(receipt.ruling_written);
        assert!(!receipt.proposed_act_written);
        assert!(receipt.proposed_act_id.is_none());
        let snapshot = world.snapshot().unwrap();
        assert_eq!(snapshot.rulings.len(), 1);
        assert!(snapshot.admitted_acts.is_empty());
    }

    #[tokio::test]
    async fn admission_service_clouds_and_commits_only_ruling() {
        let mut context = valid_context();
        context.boundaries.push(Boundary {
            boundary_id: "boundary:semantic".into(),
            kind: BoundaryKind::Semantic,
            rule: BoundaryRule {
                name: "semantic_review".into(),
                value: None,
            },
            status: "closed".into(),
        });
        let world = InMemoryAdmissionWorld::new(context);
        let service = AdmissionService::new(world.clone());

        let receipt = service.admit(&valid_act()).await.unwrap();

        assert_eq!(receipt.decision, AdmissionDecision::Cloud);
        assert!(receipt.ruling_written);
        assert!(!receipt.proposed_act_written);
        let snapshot = world.snapshot().unwrap();
        assert_eq!(snapshot.rulings.len(), 1);
        assert!(snapshot.admitted_acts.is_empty());
    }

    #[tokio::test]
    async fn admission_service_uses_evaluate_admission_not_custom_policy() {
        let mut context = valid_context();
        context.boundaries.push(Boundary {
            boundary_id: "boundary:no-host-inspect".into(),
            kind: BoundaryKind::Deterministic,
            rule: BoundaryRule {
                name: "did_must_not_be".into(),
                value: Some("host.inspect".into()),
            },
            status: "closed".into(),
        });
        let world = InMemoryAdmissionWorld::new(context);
        let service = AdmissionService::new(world.clone());

        let receipt = service.admit(&valid_act()).await.unwrap();

        assert_eq!(receipt.decision, AdmissionDecision::No);
        assert!(receipt
            .reasons
            .iter()
            .any(|reason| reason.contains("boundary:no-host-inspect")));
        let snapshot = world.snapshot().unwrap();
        assert_eq!(snapshot.rulings[0].boundaries_failed, 1);
        assert!(snapshot.admitted_acts.is_empty());
    }

    #[test]
    fn memory_world_is_not_production_adapter() {
        let world = InMemoryAdmissionWorld::new(valid_context());

        assert_eq!(world.world_name(), InMemoryAdmissionWorld::WORLD_NAME);
        assert!(!world.is_production_adapter());
    }
}
