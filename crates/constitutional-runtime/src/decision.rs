//! Deliberation layer: resolve [`IRPrimitive::Decide`] into concrete primitives, then lower.
//!
//! Runtime [`MinilabRuntimeLowerer`](crate::lowering::MinilabRuntimeLowerer) intentionally does **not**
//! lower `Decide`. This module is the constitutional place for that translation.

use crate::ir::{IRPrimitive, IrNode};
use crate::lowering::{
    Lowerer, LoweringError, LoweringPlan, MinilabRuntimeLowerer, OperationalCommand,
};
use crate::refs::NodeId;
use crate::validation::MAX_ROUTE_NESTING_DEPTH;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("Decide resolution nesting exceeded limit ({0})")]
    NestingLimitExceeded(usize),
    #[error("materialized graph still contains Decide at node {0}")]
    UnresolvedDecide(NodeId),
}

/// Supplies the chosen branch for each [`IRPrimitive::Decide`] (policy engine, LLM, human, …).
pub trait DecideResolver {
    /// `node.body` must be [`IRPrimitive::Decide`]. Return the **concrete** primitive to execute
    /// (must not be another `Decide` unless your policy allows chained deliberation).
    fn resolve_decide(&self, node: &IrNode) -> Result<IRPrimitive, PlannerError>;
}

/// Recursively replaces every `Decide` using `resolver` (bounded by [`MAX_ROUTE_NESTING_DEPTH`]).
pub fn materialize_primitive(
    p: &IRPrimitive,
    resolver: &impl DecideResolver,
    owning_id: &NodeId,
    depth: usize,
) -> Result<IRPrimitive, PlannerError> {
    if depth > MAX_ROUTE_NESTING_DEPTH {
        return Err(PlannerError::NestingLimitExceeded(MAX_ROUTE_NESTING_DEPTH));
    }
    match p {
        IRPrimitive::Decide { .. } => {
            let n = IrNode {
                id: owning_id.clone(),
                body: p.clone(),
            };
            resolver.resolve_decide(&n)
        }
        IRPrimitive::Route { operation, surface } => Ok(IRPrimitive::Route {
            operation: Box::new(materialize_primitive(
                operation,
                resolver,
                owning_id,
                depth + 1,
            )?),
            surface: surface.clone(),
        }),
        IRPrimitive::Schedule { action, trigger } => Ok(IRPrimitive::Schedule {
            action: Box::new(materialize_primitive(
                action,
                resolver,
                owning_id,
                depth + 1,
            )?),
            trigger: trigger.clone(),
        }),
        IRPrimitive::Confirm { action, role } => Ok(IRPrimitive::Confirm {
            action: Box::new(materialize_primitive(
                action,
                resolver,
                owning_id,
                depth + 1,
            )?),
            role: role.clone(),
        }),
        _ => Ok(p.clone()),
    }
}

/// Materialize a full [`IrNode`] (same recursion rules).
pub fn compile_node(node: &IrNode, resolver: &impl DecideResolver) -> Result<IrNode, PlannerError> {
    Ok(IrNode {
        id: node.id.clone(),
        body: materialize_primitive(&node.body, resolver, &node.id, 0)?,
    })
}

/// Compile a batch of top-level nodes (e.g. pipeline steps).
pub fn compile_flow(
    nodes: &[IrNode],
    resolver: &impl DecideResolver,
) -> Result<Vec<IrNode>, PlannerError> {
    nodes.iter().map(|n| compile_node(n, resolver)).collect()
}

/// True if any `Decide` remains anywhere in the primitive tree.
pub fn contains_decide(p: &IRPrimitive) -> bool {
    match p {
        IRPrimitive::Decide { .. } => true,
        IRPrimitive::Route { operation, .. } => contains_decide(operation),
        IRPrimitive::Schedule { action, .. } => contains_decide(action),
        IRPrimitive::Confirm { action, .. } => contains_decide(action),
        _ => false,
    }
}

/// Debug assert: compiled graph must be Decide-free before runtime lowering.
pub fn assert_decide_free(nodes: &[IrNode]) -> Result<(), PlannerError> {
    for n in nodes {
        if contains_decide(&n.body) {
            return Err(PlannerError::UnresolvedDecide(n.id.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PlannerLoweringError {
    #[error(transparent)]
    Planner(#[from] PlannerError),
    #[error(transparent)]
    Lowering(#[from] LoweringError),
}

/// Lower every node after successful compilation (fails if any `Decide` remains).
pub fn lower_compiled_flow(
    lowerer: &MinilabRuntimeLowerer,
    nodes: &[IrNode],
) -> Result<Vec<(LoweringPlan, OperationalCommand)>, PlannerLoweringError> {
    assert_decide_free(nodes)?;
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        out.push(lowerer.lower(n)?);
    }
    Ok(out)
}

/// Resolve → compile single node → lower (convenience for one-step deliberation).
pub fn resolve_lower_one(
    lowerer: &MinilabRuntimeLowerer,
    node: &IrNode,
    resolver: &impl DecideResolver,
) -> Result<(LoweringPlan, OperationalCommand), PlannerLoweringError> {
    let compiled = compile_node(node, resolver)?;
    assert_decide_free(std::slice::from_ref(&compiled))?;
    Ok(lowerer.lower(&compiled)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refs::{DataRef, PolicyId, TargetRef};

    struct AlwaysObserve;

    impl DecideResolver for AlwaysObserve {
        fn resolve_decide(&self, _node: &IrNode) -> Result<IRPrimitive, PlannerError> {
            Ok(IRPrimitive::Observe {
                target: TargetRef("resolved".into()),
                scope: "facts".into(),
            })
        }
    }

    #[test]
    fn compile_replaces_decide() {
        let n = IrNode {
            id: NodeId("d1".into()),
            body: IRPrimitive::Decide {
                context: DataRef("ctx".into()),
                policy: PolicyId("p1".into()),
            },
        };
        let out = compile_node(&n, &AlwaysObserve).unwrap();
        assert!(!contains_decide(&out.body));
        assert!(matches!(out.body, IRPrimitive::Observe { .. }));
    }

    #[test]
    fn lower_after_compile() {
        let n = IrNode {
            id: NodeId("d2".into()),
            body: IRPrimitive::Decide {
                context: DataRef("ctx".into()),
                policy: PolicyId("p1".into()),
            },
        };
        let lowerer = MinilabRuntimeLowerer;
        let (plan, cmd) = resolve_lower_one(&lowerer, &n, &AlwaysObserve).unwrap();
        assert_eq!(cmd.namespace, "host");
        assert_eq!(cmd.verb, "inspect");
        assert!(!plan.evidence.required_kinds.is_empty());
    }
}
