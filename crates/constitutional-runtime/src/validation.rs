//! Three-pass constitutional admissibility: structure, policy, capability.

use crate::capability::{primitive_kind, CapabilityManifest, PrimitiveName};
use crate::ir::{IRPrimitive, IrNode};
use crate::policy::PolicyClass;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum nesting depth for `Route` / `Schedule` / `Confirm` inner graphs.
pub const MAX_ROUTE_NESTING_DEPTH: usize = 4;

/// Context for policy and evidence realizability (kept **outside** IR shapes).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissibilityContext {
    pub policy_class: PolicyClass,
    /// If false, the runtime must not touch this node.
    pub runtime_permitted: bool,
    /// True when validating for runtime execution (vs planning-only graph).
    pub at_execution_boundary: bool,
    /// If true, substrates must declare `evidence.write` in [`CapabilityManifest::declared_guarantees`].
    pub require_evidence_closure: bool,
}

impl Default for AdmissibilityContext {
    fn default() -> Self {
        Self {
            policy_class: PolicyClass::C,
            runtime_permitted: true,
            at_execution_boundary: true,
            require_evidence_closure: true,
        }
    }
}

/// Node that passed all three passes (clone of [`IrNode`] for explicit typing).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissibleNode {
    pub node: IrNode,
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("structure: {0}")]
    Structure(String),
    #[error("policy: {0}")]
    Policy(String),
    #[error("capability: {0}")]
    Capability(String),
}

fn max_nesting_depth(prim: &IRPrimitive) -> usize {
    match prim {
        IRPrimitive::Route { operation, .. } => 1 + max_nesting_depth(operation),
        // `Schedule` and `Confirm` both wrap a single inner action graph, so
        // structural depth is counted through `action` with the same rule.
        IRPrimitive::Schedule { action, .. } | IRPrimitive::Confirm { action, .. } => {
            1 + max_nesting_depth(action)
        }
        _ => 1,
    }
}

fn non_empty(s: &str, field: &str) -> Result<(), ValidationError> {
    if s.trim().is_empty() {
        return Err(ValidationError::Structure(format!(
            "{field} must be non-empty"
        )));
    }
    Ok(())
}

/// Pass 1 — primitive-specific shape and coherence.
pub fn validate_structure(node: &IrNode) -> Result<(), ValidationError> {
    if non_empty(&node.id.0, "node.id").is_err() {
        return Err(ValidationError::Structure(
            "node.id must be non-empty".into(),
        ));
    }
    if max_nesting_depth(&node.body) > MAX_ROUTE_NESTING_DEPTH {
        return Err(ValidationError::Structure(format!(
            "route/schedule/confirm nesting exceeds depth {}",
            MAX_ROUTE_NESTING_DEPTH
        )));
    }

    match &node.body {
        IRPrimitive::Observe { target, scope } => {
            non_empty(&target.0, "target")?;
            non_empty(scope, "scope")?;
        }
        IRPrimitive::Collect {
            kind,
            target,
            window,
        } => {
            non_empty(&kind.0, "kind")?;
            non_empty(&target.0, "target")?;
            non_empty(&window.0, "window")?;
        }
        IRPrimitive::Fetch { kind, id } => {
            non_empty(&kind.0, "kind")?;
            non_empty(id, "id")?;
        }
        IRPrimitive::Compress {
            kind, input_ref, ..
        } => {
            non_empty(&kind.0, "kind")?;
            non_empty(&input_ref.0, "input_ref")?;
        }
        IRPrimitive::Classify {
            kind,
            input_ref,
            schema,
            ..
        } => {
            non_empty(&kind.0, "kind")?;
            non_empty(&input_ref.0, "input_ref")?;
            non_empty(&schema.0, "schema")?;
        }
        IRPrimitive::Prioritize {
            kind,
            input_ref,
            policy,
            ..
        } => {
            non_empty(&kind.0, "kind")?;
            non_empty(&input_ref.0, "input_ref")?;
            non_empty(&policy.0, "policy")?;
        }
        IRPrimitive::Compare { kind, left, right } => {
            non_empty(&kind.0, "kind")?;
            non_empty(&left.0, "left")?;
            non_empty(&right.0, "right")?;
        }
        IRPrimitive::Decide { context, policy } => {
            non_empty(&context.0, "context")?;
            non_empty(&policy.0, "policy")?;
        }
        IRPrimitive::Route { operation, surface } => {
            non_empty(&surface.0, "surface")?;
            validate_structure(&IrNode {
                id: node.id.clone(),
                body: (**operation).clone(),
            })?;
        }
        IRPrimitive::Schedule { action, trigger } => {
            non_empty(&trigger.0, "trigger")?;
            validate_structure(&IrNode {
                id: node.id.clone(),
                body: (**action).clone(),
            })?;
        }
        IRPrimitive::Execute { .. } => {}
        IRPrimitive::Reconcile {
            target, desired, ..
        } => {
            non_empty(&target.0, "target")?;
            non_empty(&desired.0, "desired")?;
        }
        IRPrimitive::Emit { surface, payload } => {
            non_empty(&surface.0, "surface")?;
            non_empty(&payload.0, "payload")?;
        }
        IRPrimitive::Persist { data, .. } => {
            non_empty(&data.0, "data")?;
        }
        IRPrimitive::Confirm { action, role } => {
            non_empty(&role.0, "role")?;
            validate_structure(&IrNode {
                id: node.id.clone(),
                body: (**action).clone(),
            })?;
        }
        IRPrimitive::Cancel { id } => {
            non_empty(id, "id")?;
        }
    }
    Ok(())
}

fn policy_allows_primitive(class: PolicyClass, p: &IRPrimitive) -> bool {
    match class {
        PolicyClass::A => matches!(
            p,
            IRPrimitive::Observe { .. }
                | IRPrimitive::Collect { .. }
                | IRPrimitive::Fetch { .. }
                | IRPrimitive::Compress { .. }
                | IRPrimitive::Classify { .. }
                | IRPrimitive::Prioritize { .. }
                | IRPrimitive::Compare { .. }
                | IRPrimitive::Emit { .. }
                | IRPrimitive::Route { .. }
                | IRPrimitive::Schedule { .. }
        ),
        PolicyClass::B => !matches!(p, IRPrimitive::Reconcile { .. }),
        // C and D both allow the full primitive set for *representation* checks; D is blocked
        // at execution time by the dedicated rule below (not by this matrix).
        PolicyClass::C | PolicyClass::D => true,
    }
}

/// Pass 2 — policy class, runtime permission, execution boundary.
pub fn validate_policy(node: &IrNode, ctx: &AdmissibilityContext) -> Result<(), ValidationError> {
    if !ctx.runtime_permitted {
        return Err(ValidationError::Policy(
            "runtime not permitted to touch this node".into(),
        ));
    }
    if ctx.at_execution_boundary && matches!(node.body, IRPrimitive::Decide { .. }) {
        return Err(ValidationError::Policy(
            "Decide is not executable at the runtime boundary; use a planner".into(),
        ));
    }
    if ctx.at_execution_boundary && ctx.policy_class == PolicyClass::D {
        return Err(ValidationError::Policy(
            "PolicyClass D is reserved: not executable at the normal runtime boundary; use deliberation, human authority, or an explicit exception path".into(),
        ));
    }
    if !policy_allows_primitive(ctx.policy_class, &node.body) {
        return Err(ValidationError::Policy(format!(
            "primitive {:?} not allowed under policy class {:?}",
            PrimitiveName::from_primitive(&node.body),
            ctx.policy_class
        )));
    }
    Ok(())
}

/// Pass 3 — substrate support, kind constraints, evidence realizability.
pub fn validate_capability(
    node: &IrNode,
    manifests: &[CapabilityManifest],
    ctx: &AdmissibilityContext,
) -> Result<(), ValidationError> {
    if manifests.is_empty() {
        return Err(ValidationError::Capability(
            "no capability manifests provided".into(),
        ));
    }
    let mut any = false;
    for m in manifests {
        if !m.can_realize(&node.body) {
            continue;
        }
        if !m.kind_allowed(&node.body) {
            continue;
        }
        if !m.evidence_realizable(ctx.require_evidence_closure) {
            continue;
        }
        any = true;
        break;
    }
    if !any {
        return Err(ValidationError::Capability(format!(
            "no substrate can realize {:?} with kind={:?} and evidence requirements (manifests checked: {})",
            PrimitiveName::from_primitive(&node.body),
            primitive_kind(&node.body),
            manifests.len()
        )));
    }
    Ok(())
}

/// Constitutional gate: structure → policy → capability.
pub fn validate_admissibility(
    node: &IrNode,
    manifests: &[CapabilityManifest],
    ctx: &AdmissibilityContext,
) -> Result<AdmissibleNode, ValidationError> {
    validate_structure(node)?;
    validate_policy(node, ctx)?;
    validate_capability(node, manifests, ctx)?;
    Ok(AdmissibleNode { node: node.clone() })
}

/// Back-compat: single-manifest capability check.
pub fn check_capability(
    manifest: &CapabilityManifest,
    node: &IrNode,
) -> Result<(), ValidationError> {
    let ctx = AdmissibilityContext {
        require_evidence_closure: false,
        ..Default::default()
    };
    validate_capability(node, std::slice::from_ref(manifest), &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Kind, Window};
    use crate::refs::{NodeId, TargetRef};
    use std::collections::BTreeSet;

    fn manifest_all() -> CapabilityManifest {
        CapabilityManifest {
            substrate_id: "local".into(),
            substrate_version: "1".into(),
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

    #[test]
    fn inadmissible_empty_collect() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Collect {
                kind: Kind("".into()),
                target: TargetRef("t".into()),
                window: Window("24h".into()),
            },
        };
        assert!(validate_structure(&node).is_err());
    }

    #[test]
    fn policy_blocks_execute_under_a() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Execute {
                action: crate::ir::ActionKind::Canonical(
                    crate::act_identity::CanonicalActionId::new("x", "y").unwrap(),
                ),
                params: Default::default(),
            },
        };
        let ctx = AdmissibilityContext {
            policy_class: PolicyClass::A,
            ..Default::default()
        };
        assert!(validate_policy(&node, &ctx).is_err());
    }

    #[test]
    fn capability_mismatch() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Observe {
                target: TargetRef("h".into()),
                scope: "s".into(),
            },
        };
        let mut m = manifest_all();
        m.supported_primitives.remove(&PrimitiveName::Observe);
        let ctx = AdmissibilityContext::default();
        assert!(validate_capability(&node, &[m], &ctx).is_err());
    }

    #[test]
    fn admissible_collect() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Collect {
                kind: Kind("events".into()),
                target: TargetRef("lab8gb".into()),
                window: Window("24h".into()),
            },
        };
        let ctx = AdmissibilityContext::default();
        validate_admissibility(&node, &[manifest_all()], &ctx).unwrap();
    }

    #[test]
    fn policy_d_blocked_at_runtime_boundary() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Collect {
                kind: Kind("events".into()),
                target: TargetRef("h".into()),
                window: Window("1h".into()),
            },
        };
        let ctx = AdmissibilityContext {
            policy_class: PolicyClass::D,
            at_execution_boundary: true,
            ..Default::default()
        };
        let e = validate_policy(&node, &ctx).unwrap_err();
        assert!(matches!(e, ValidationError::Policy(_)));
        let msg = e.to_string();
        assert!(
            msg.contains("PolicyClass D") || msg.contains("reserved"),
            "expected D semantics in message: {msg}"
        );
    }

    #[test]
    fn policy_d_allowed_in_planning_graph_only() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Collect {
                kind: Kind("events".into()),
                target: TargetRef("h".into()),
                window: Window("1h".into()),
            },
        };
        let ctx = AdmissibilityContext {
            policy_class: PolicyClass::D,
            at_execution_boundary: false,
            ..Default::default()
        };
        validate_policy(&node, &ctx).unwrap();
    }

    #[test]
    fn policy_a_blocks_persist_and_confirm() {
        let persist = IrNode {
            id: NodeId("p".into()),
            body: IRPrimitive::Persist {
                data: crate::refs::DataRef("d".into()),
                durability: crate::ir::DurabilityClass::Durable,
            },
        };
        let confirm = IrNode {
            id: NodeId("c".into()),
            body: IRPrimitive::Confirm {
                action: Box::new(IRPrimitive::Observe {
                    target: TargetRef("h".into()),
                    scope: "s".into(),
                }),
                role: crate::ir::Role("admin".into()),
            },
        };
        let ctx = AdmissibilityContext {
            policy_class: PolicyClass::A,
            ..Default::default()
        };
        assert!(validate_policy(&persist, &ctx).is_err());
        assert!(validate_policy(&confirm, &ctx).is_err());
    }

    #[test]
    fn runtime_not_permitted() {
        let node = IrNode {
            id: NodeId("n".into()),
            body: IRPrimitive::Observe {
                target: TargetRef("h".into()),
                scope: "s".into(),
            },
        };
        let ctx = AdmissibilityContext {
            runtime_permitted: false,
            ..Default::default()
        };
        assert!(validate_policy(&node, &ctx).is_err());
    }
}
