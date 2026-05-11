//! Planning compiler: surface → IR graph → admissibility → lowered commands.
//!
//! This is the **costura** the operational grammar needed. It closes the
//! constitutional pipeline:
//!
//! ```text
//! OperationalProgram
//!   -> normalize
//!   -> line/tree → IR (per-entry, via OperationalLine::to_ir_primitive)
//!   -> assign deterministic node ids
//!   -> build parent/child IR graph
//!   -> validate_admissibility (structure · policy · capability)
//!   -> Lowerer::lower per admissible node
//!   -> CompiledOperationalPlan { graph, per-node plan, commands }
//! ```
//!
//! It is a **compiler**, not a planner in the LLM / workflow-engine sense:
//!
//! - no side effects,
//! - no mutation of the program,
//! - no implicit `Decide` resolution,
//! - no dispatch,
//! - no rewrites of nested primitives beyond what the surface already encodes.
//!
//! The output [`CompiledOperationalPlan`] is a serializable artifact that the
//! runtime can inspect, diff, snapshot, replay, and eventually dispatch. The
//! artifact itself does nothing.
//!
//! ## Deterministic ids
//!
//! Node ids are derived **purely from structural position**:
//!
//! - top-level entry `i` → `n{i}` (e.g. `n0`, `n1`, …)
//! - child `j` of entry `i` → `n{i}.c{j}`
//!
//! Same normalized program → same ids, byte-for-byte. This is the property
//! that makes replay, diff, simulation, and explainability work.
//!
//! ## Closed error sets
//!
//! [`CompileError`] covers pre-admissibility failures (malformed surface or
//! unmapped verb). [`PlanError`] wraps those plus admissibility /
//! lowering failures with the offending [`NodeId`]. Neither returns a free
//! string from the runtime boundary.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::capability::CapabilityManifest;
use crate::failure::{PolicyClassTag, RuntimeFailure};
use crate::idempotency::{IdempotencyClass, IdempotencyContract, ReplayStance};
use crate::ir::{ActionKind, IRPrimitive, InferSurface, IrNode};
use crate::lowering::{Lowerer, LoweringError, LoweringPlan, OperationalCommand};
use crate::operational_grammar::{
    IrLoweringError, OperationalEntry, OperationalProgram, ParseError,
};
use crate::policy::PolicyClass;
use crate::refs::NodeId;
use crate::validation::{validate_admissibility, AdmissibilityContext, ValidationError};

// -------------------------------------------------------------------------
// Graph + plan artifacts
// -------------------------------------------------------------------------

/// Parent/child edge between two IR nodes as they appeared in the source
/// program. We intentionally do **not** encode semantic data-flow here —
/// those come from the IR primitive's own boxed fields (`Confirm.action`,
/// `Schedule.action`, `Route.operation`). An edge means "child was
/// syntactically nested under parent in the surface".
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub parent: NodeId,
    pub child: NodeId,
}

/// A flat, ordered IR graph. `nodes` are in deterministic program order
/// (top-level entries followed by their children, depth-first). `edges`
/// record the syntactic parent/child relation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IrGraph {
    pub nodes: Vec<IrNode>,
    pub edges: Vec<Edge>,
}

impl IrGraph {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn find(&self, id: &NodeId) -> Option<&IrNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }
}

/// Per-node planning outcome. One of these is produced for every admissible
/// node. Nodes that fail admissibility or lowering abort the whole plan —
/// the artifact is all-or-nothing at PR 3.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodePlan {
    pub node_id: NodeId,
    pub primitive: IRPrimitive,
    pub lowering: LoweringPlan,
    pub command: OperationalCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_infer_surface: Option<InferSurface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_substrate_id: Option<String>,
    /// PR 5c: inferred idempotency contract for this node. The planner
    /// populates it from the IR primitive (and, for `Execute`, from the
    /// action name). Dispatchers and schedulers use this to decide retry /
    /// replay stance; absence means "no declared contract" — callers must
    /// either treat it conservatively or abort per their policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency: Option<IdempotencyContract>,
}

/// Full planning artifact. Serializable, deterministic, inspectable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompiledOperationalPlan {
    pub program: OperationalProgram,
    pub graph: IrGraph,
    pub node_plans: Vec<NodePlan>,
}

impl CompiledOperationalPlan {
    pub fn commands(&self) -> impl Iterator<Item = &OperationalCommand> {
        self.node_plans.iter().map(|p| &p.command)
    }
}

// -------------------------------------------------------------------------
// Errors
// -------------------------------------------------------------------------

/// Errors before admissibility: the surface was malformed or the verb has no
/// IR mapping today. These are structural, not constitutional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    /// Post-parse normalization failed.
    Normalize(ParseError),
    /// Surface line could not be lowered to an IR primitive. `entry_path` is
    /// the same structural path used to derive node ids (e.g. `"n2.c0"`),
    /// so the caller can point at the offending line without string matching.
    IrLowering {
        entry_path: String,
        error: IrLoweringError,
    },
    /// Strong Grammar JSON parsing failed before IR construction.
    StrongSyntaxError(String),
    /// Strong Grammar parsed but could not be mapped into canonical IR.
    StrongLoweringError(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Normalize(e) => write!(f, "normalize: {e}"),
            CompileError::IrLowering { entry_path, error } => {
                write!(f, "ir-lowering at {entry_path}: {error}")
            }
            CompileError::StrongSyntaxError(error) => write!(f, "strong-syntax: {error}"),
            CompileError::StrongLoweringError(error) => write!(f, "strong-lowering: {error}"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Errors from the full planning pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    Compile(CompileError),
    /// Admissibility (structure / policy / capability) blocked a node.
    ///
    /// `reason` is the back-compat string (prefixed with `structure:` /
    /// `policy:` / `capability:`); `runtime_failure` is the PR 5e
    /// structured form — stage-tagged, consumable without parsing.
    ValidationBlocked {
        node_id: NodeId,
        reason: String,
        runtime_failure: RuntimeFailure,
    },
    /// Lowering itself refused to translate the primitive (typically
    /// `IRPrimitive::Decide`, which is representation-only).
    LoweringUnavailable {
        node_id: NodeId,
        reason: String,
        runtime_failure: RuntimeFailure,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::Compile(e) => write!(f, "compile: {e}"),
            PlanError::ValidationBlocked {
                node_id, reason, ..
            } => {
                write!(f, "validation blocked {}: {}", node_id, reason)
            }
            PlanError::LoweringUnavailable {
                node_id, reason, ..
            } => {
                write!(f, "lowering unavailable for {}: {}", node_id, reason)
            }
        }
    }
}

impl std::error::Error for PlanError {}

impl From<CompileError> for PlanError {
    fn from(e: CompileError) -> Self {
        PlanError::Compile(e)
    }
}

// -------------------------------------------------------------------------
// Compile: program → IR graph
// -------------------------------------------------------------------------

/// Normalize the program and compile every entry into an [`IrNode`], stamping
/// deterministic ids and recording parent/child edges. Does **not** validate
/// admissibility and does **not** lower — see [`plan_operational_program`]
/// for the end-to-end path.
pub fn compile_program_to_ir_graph(program: &OperationalProgram) -> Result<IrGraph, CompileError> {
    program.normalize().map_err(CompileError::Normalize)?;

    let mut graph = IrGraph::default();

    for (i, entry) in program.entries.iter().enumerate() {
        let parent_id = top_level_id(i);
        let parent_node = compile_entry(&parent_id, entry)?;
        graph.nodes.push(parent_node);

        for (j, child) in entry.children.iter().enumerate() {
            let child_id = child_id(i, j);
            let child_node = compile_entry(&child_id, child)?;
            graph.edges.push(Edge {
                parent: parent_id.clone(),
                child: child_id.clone(),
            });
            graph.nodes.push(child_node);
        }
    }

    Ok(graph)
}

fn compile_entry(id: &NodeId, entry: &OperationalEntry) -> Result<IrNode, CompileError> {
    let primitive = entry
        .line
        .to_ir_primitive()
        .map_err(|e| CompileError::IrLowering {
            entry_path: id.0.clone(),
            error: e,
        })?;
    Ok(IrNode {
        id: id.clone(),
        body: primitive,
    })
}

fn top_level_id(i: usize) -> NodeId {
    NodeId(format!("n{i}"))
}

fn child_id(i: usize, j: usize) -> NodeId {
    NodeId(format!("n{i}.c{j}"))
}

// -------------------------------------------------------------------------
// Plan: compile + validate + lower
// -------------------------------------------------------------------------

/// End-to-end compile: surface → admissible IR graph → commands.
///
/// The call is **pure**. On success returns an inspectable
/// [`CompiledOperationalPlan`]; on any failure returns a closed [`PlanError`]
/// with the offending [`NodeId`]. The caller decides what to do with the
/// plan artifact (dispatch, log, diff, replay, drop).
pub fn plan_operational_program(
    program: &OperationalProgram,
    manifests: &[CapabilityManifest],
    ctx: &AdmissibilityContext,
    lowerer: &dyn Lowerer,
) -> Result<CompiledOperationalPlan, PlanError> {
    let graph = compile_program_to_ir_graph(program)?;
    plan_ir_graph(graph, program.clone(), manifests, ctx, lowerer)
}

/// End-to-end plan for an already-compiled IR graph.
///
/// This is the companion boundary used by Strong Grammar and other non-textual
/// ingress surfaces: the caller has already produced canonical IR, but the
/// graph still must pass the same admissibility, routing, lowering, and
/// all-or-nothing planning gates as operational grammar programs.
pub fn plan_ir_graph(
    graph: IrGraph,
    program: OperationalProgram,
    manifests: &[CapabilityManifest],
    ctx: &AdmissibilityContext,
    lowerer: &dyn Lowerer,
) -> Result<CompiledOperationalPlan, PlanError> {
    let mut node_plans = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        validate_admissibility(node, manifests, ctx).map_err(|e| {
            let reason = validation_reason(&e);
            let runtime_failure = validation_error_to_runtime_failure(&e, node, ctx);
            PlanError::ValidationBlocked {
                node_id: node.id.clone(),
                reason,
                runtime_failure,
            }
        })?;

        let routed = route_infer_surface(node, manifests, ctx).map_err(|e| {
            let reason = e.to_string();
            let runtime_failure = match &e {
                InferRoutingError::CapabilityUnavailable {
                    runtime_failure, ..
                } => runtime_failure.clone(),
            };
            PlanError::ValidationBlocked {
                node_id: node.id.clone(),
                reason,
                runtime_failure,
            }
        })?;

        let lowered_node = IrNode {
            id: node.id.clone(),
            body: routed.primitive.clone(),
        };

        let (lowering, command) = lowerer.lower(&lowered_node).map_err(|e: LoweringError| {
            let reason = e.to_string();
            let runtime_failure = RuntimeFailure::Lowering {
                node_id: node.id.0.clone(),
                target_runtime: "operational".into(),
                reason_code: "lowerer_refused".into(),
                detail: reason.clone(),
            };
            PlanError::LoweringUnavailable {
                node_id: node.id.clone(),
                reason,
                runtime_failure,
            }
        })?;

        node_plans.push(NodePlan {
            node_id: node.id.clone(),
            primitive: routed.primitive.clone(),
            lowering,
            command,
            selected_infer_surface: routed.selected_infer_surface,
            selected_substrate_id: routed.selected_substrate_id,
            idempotency: infer_idempotency_for_primitive(&node.body),
        });
    }

    Ok(CompiledOperationalPlan {
        program: program.clone(),
        graph,
        node_plans,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RoutedPrimitive {
    primitive: IRPrimitive,
    selected_infer_surface: Option<InferSurface>,
    selected_substrate_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum InferRoutingError {
    CapabilityUnavailable {
        reason: String,
        runtime_failure: RuntimeFailure,
    },
}

impl fmt::Display for InferRoutingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferRoutingError::CapabilityUnavailable { reason, .. } => f.write_str(reason),
        }
    }
}

fn route_infer_surface(
    node: &IrNode,
    manifests: &[CapabilityManifest],
    ctx: &AdmissibilityContext,
) -> Result<RoutedPrimitive, InferRoutingError> {
    let requested = requested_infer_surface(&node.body);

    if let Some(surface) = requested.clone().filter(|s| !s.is_unspecified_default()) {
        return if let Some(substrate_id) =
            find_manifest_for_infer_surface(manifests, &surface, &node.body, ctx)
        {
            Ok(RoutedPrimitive {
                primitive: with_infer_surface(node.body.clone(), surface.clone()),
                selected_infer_surface: Some(surface),
                selected_substrate_id: Some(substrate_id),
            })
        } else {
            Err(infer_surface_capability_error(node, &surface))
        };
    }

    if should_prefer_lab512(&node.body) {
        let surface = InferSurface::Lab512;
        if let Some(substrate_id) =
            find_manifest_for_infer_surface(manifests, &surface, &node.body, ctx)
        {
            return Ok(RoutedPrimitive {
                primitive: with_infer_surface(node.body.clone(), surface.clone()),
                selected_infer_surface: Some(surface),
                selected_substrate_id: Some(substrate_id),
            });
        }
    }

    Ok(RoutedPrimitive {
        primitive: node.body.clone(),
        selected_infer_surface: None,
        selected_substrate_id: None,
    })
}

fn requested_infer_surface(p: &IRPrimitive) -> Option<InferSurface> {
    match p {
        IRPrimitive::Compress { infer_surface, .. }
        | IRPrimitive::Classify { infer_surface, .. }
        | IRPrimitive::Prioritize { infer_surface, .. } => Some(infer_surface.clone()),
        IRPrimitive::Execute {
            action: ActionKind::Canonical(id),
            params,
        } if is_infer_routable_execute_action(id.dotted_str()) => params
            .get("infer")
            .and_then(|v| v.as_str())
            .and_then(parse_infer_surface_param),
        _ => None,
    }
}

fn should_prefer_lab512(p: &IRPrimitive) -> bool {
    matches!(
        p,
        IRPrimitive::Compress { .. }
            | IRPrimitive::Classify { .. }
            | IRPrimitive::Prioritize { .. }
    ) || matches!(
        p,
        IRPrimitive::Execute {
            action: ActionKind::Canonical(id),
            ..
        } if is_infer_routable_execute_action(id.dotted_str())
    )
}

fn with_infer_surface(p: IRPrimitive, infer_surface: InferSurface) -> IRPrimitive {
    match p {
        IRPrimitive::Compress {
            kind, input_ref, ..
        } => IRPrimitive::Compress {
            kind,
            input_ref,
            infer_surface,
        },
        IRPrimitive::Classify {
            kind,
            input_ref,
            schema,
            ..
        } => IRPrimitive::Classify {
            kind,
            input_ref,
            schema,
            infer_surface,
        },
        IRPrimitive::Prioritize {
            kind,
            input_ref,
            policy,
            ..
        } => IRPrimitive::Prioritize {
            kind,
            input_ref,
            policy,
            infer_surface,
        },
        IRPrimitive::Execute { action, mut params } => {
            if let ActionKind::Canonical(id) = &action {
                if is_infer_routable_execute_action(id.dotted_str()) {
                    params.insert(
                        "infer".into(),
                        serde_json::Value::String(infer_surface_wire(&infer_surface).into()),
                    );
                }
            }
            IRPrimitive::Execute { action, params }
        }
        other => other,
    }
}

fn is_infer_routable_execute_action(action: &str) -> bool {
    matches!(
        action,
        "lab.summary"
            | "lab.drift"
            | "lab.route"
            | "lab.organize"
            | "flow.verify_report"
            | "flow.drift_review"
            | "flow.recover_cmd"
    )
}

fn parse_infer_surface_param(raw: &str) -> Option<InferSurface> {
    match raw {
        "local" => Some(InferSurface::Local),
        "lab256" => Some(InferSurface::Lab256),
        "lab8gb" => Some(InferSurface::Lab8gb),
        "lab512" => Some(InferSurface::Lab512),
        "cloud" => Some(InferSurface::Cloud),
        "hybrid" => Some(InferSurface::Hybrid),
        _ => None,
    }
}

fn infer_surface_wire(surface: &InferSurface) -> &'static str {
    match surface {
        InferSurface::Local => "local",
        InferSurface::Lab256 => "lab256",
        InferSurface::Lab8gb => "lab8gb",
        InferSurface::Lab512 => "lab512",
        InferSurface::Cloud => "cloud",
        InferSurface::Hybrid => "hybrid",
    }
}

fn find_manifest_for_infer_surface(
    manifests: &[CapabilityManifest],
    surface: &InferSurface,
    primitive: &IRPrimitive,
    ctx: &AdmissibilityContext,
) -> Option<String> {
    let substrate_id = surface.substrate_id()?;
    manifests
        .iter()
        .find(|m| {
            m.substrate_id == substrate_id
                && m.can_realize(primitive)
                && m.kind_allowed(primitive)
                && m.evidence_realizable(ctx.require_evidence_closure)
        })
        .map(|m| m.substrate_id.clone())
}

fn infer_surface_capability_error(node: &IrNode, surface: &InferSurface) -> InferRoutingError {
    use crate::capability::{primitive_kind, PrimitiveName};
    let substrate = surface.substrate_id().unwrap_or("unspecified").to_string();
    let reason = format!(
        "capability: requested infer surface {} cannot realize {:?} with kind={:?}",
        substrate,
        PrimitiveName::from_primitive(&node.body),
        primitive_kind(&node.body)
    );
    InferRoutingError::CapabilityUnavailable {
        runtime_failure: RuntimeFailure::Capability {
            primitive: format!("{:?}", PrimitiveName::from_primitive(&node.body)),
            kind: primitive_kind(&node.body).map(str::to_owned),
            attempted_substrate: Some(substrate.clone()),
            reason_code: "infer_surface_unavailable".into(),
            detail: reason.clone(),
        },
        reason,
    }
}

/// Infer the declared idempotency contract for an IR primitive. Covers the
/// landed vocabulary today:
///
/// - **Observe / Collect / Fetch** → `PureRead` (read-only).
/// - **Compress / Classify / Prioritize / Compare** → `DeterministicRecompute`
///   (derived-state computations).
/// - **Execute `host.pair`** → `DeduplicatedMaterialAction` with
///   `ReplayRejection` (the slice writes `replay_detected` on duplicates).
/// - **Execute `outbound.send`** → `DeduplicatedMaterialAction` with
///   `SilentSuccess` (the provider is idempotent by correlation_id).
///
/// Other primitives return `None` — the planner surfaces "no declared
/// contract" honestly rather than guessing. New Execute verbs must land
/// with their contract in the same PR (ADR 09 §Proibições explícitas #3).
pub fn infer_idempotency_for_primitive(p: &IRPrimitive) -> Option<IdempotencyContract> {
    use IRPrimitive::*;
    match p {
        Observe { .. } | Collect { .. } | Fetch { .. } => Some(IdempotencyContract::pure_read()),
        Compress { .. } | Classify { .. } | Prioritize { .. } | Compare { .. } => {
            Some(IdempotencyContract::deterministic_recompute())
        }
        Execute { action, .. } => {
            let ActionKind::Canonical(id) = action else {
                return None;
            };
            match id.dotted_str() {
                "outbound.send" => Some(IdempotencyContract {
                    class: IdempotencyClass::DeduplicatedMaterialAction,
                    replay_stance: ReplayStance::SilentSuccess,
                    key_shape: Some("correlation_id".into()),
                    tags: vec!["provider_managed".into()],
                }),
                "host.pair" => Some(IdempotencyContract {
                    class: IdempotencyClass::DeduplicatedMaterialAction,
                    replay_stance: ReplayStance::ReplayRejection,
                    key_shape: Some("sha256(host_id || challenge)".into()),
                    tags: vec!["runtime_managed".into()],
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn validation_reason(e: &ValidationError) -> String {
    // Preserve the stage tag in the reason so upstream consumers can route
    // without parsing free text. The `ValidationError` variants already
    // Display with a `"structure: …"` / `"policy: …"` / `"capability: …"`
    // prefix; we keep that contract.
    e.to_string()
}

/// PR 5e: project a [`ValidationError`] into the structured
/// [`RuntimeFailure`] vocabulary with the correct stage tag.
fn validation_error_to_runtime_failure(
    e: &ValidationError,
    node: &IrNode,
    ctx: &AdmissibilityContext,
) -> RuntimeFailure {
    use crate::capability::{primitive_kind, PrimitiveName};
    match e {
        ValidationError::Structure(msg) => RuntimeFailure::Validation {
            at: "ir".into(),
            field: None,
            detail: msg.clone(),
            reason_code: "structure_invalid".into(),
        },
        ValidationError::Policy(msg) => RuntimeFailure::Policy {
            class: policy_class_to_tag(ctx.policy_class),
            reason_code: "policy_blocked".into(),
            detail: msg.clone(),
        },
        ValidationError::Capability(msg) => RuntimeFailure::Capability {
            primitive: format!("{:?}", PrimitiveName::from_primitive(&node.body)),
            kind: primitive_kind(&node.body).map(str::to_owned),
            attempted_substrate: None,
            reason_code: "capability_unsatisfied".into(),
            detail: msg.clone(),
        },
    }
}

/// [`RuntimeFailure::Policy::class`] is a closed `A`/`B`/`C` tag. Policy
/// class `D` is representation-only (never executable at the runtime
/// boundary) so it maps to `C` for the structured failure — the concrete
/// blocker detail is preserved in the `detail` field.
fn policy_class_to_tag(c: PolicyClass) -> PolicyClassTag {
    match c {
        PolicyClass::A => PolicyClassTag::A,
        PolicyClass::B => PolicyClassTag::B,
        PolicyClass::C | PolicyClass::D => PolicyClassTag::C,
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::PrimitiveName;
    use crate::lowering::MinilabRuntimeLowerer;
    use crate::operational_grammar::parse_program;
    use crate::policy::PolicyClass;
    use std::collections::BTreeSet;

    fn manifest_full() -> CapabilityManifest {
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

    fn infer_manifest(substrate_id: &str, primitives: &[PrimitiveName]) -> CapabilityManifest {
        CapabilityManifest {
            substrate_id: substrate_id.into(),
            substrate_version: "1".into(),
            supported_primitives: BTreeSet::from_iter(primitives.iter().cloned()),
            declared_guarantees: BTreeSet::from(["evidence.write".into()]),
            ..Default::default()
        }
    }

    fn exec_infer_manifest(substrate_id: &str, action: &str) -> CapabilityManifest {
        CapabilityManifest {
            substrate_id: substrate_id.into(),
            substrate_version: "1".into(),
            supported_primitives: BTreeSet::from([PrimitiveName::Execute]),
            supported_kinds: BTreeSet::from([action.to_string()]),
            declared_guarantees: BTreeSet::from(["evidence.write".into()]),
            ..Default::default()
        }
    }

    fn ctx() -> AdmissibilityContext {
        AdmissibilityContext {
            policy_class: PolicyClass::C,
            runtime_permitted: true,
            at_execution_boundary: true,
            require_evidence_closure: true,
        }
    }

    // ------- compile_program_to_ir_graph -------

    #[test]
    fn compile_assigns_deterministic_ids() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\nlab.compress kind=drift target=core\n",
        )
        .unwrap();
        let g = compile_program_to_ir_graph(&program).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].id.0, "n0");
        assert_eq!(g.nodes[1].id.0, "n1");
        assert!(g.edges.is_empty());
    }

    #[test]
    fn compile_records_parent_child_edges_for_indented_children() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\n  host.inspect target=lab8gb\n  host.facts target=lab8gb\nhost.verify target=core\n",
        )
        .unwrap();
        let g = compile_program_to_ir_graph(&program).unwrap();
        // 4 total: n0, n0.c0, n0.c1, n1
        let ids: Vec<_> = g.nodes.iter().map(|n| n.id.0.clone()).collect();
        assert_eq!(ids, vec!["n0", "n0.c0", "n0.c1", "n1"]);
        let edges: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.parent.0.clone(), e.child.0.clone()))
            .collect();
        assert_eq!(
            edges,
            vec![("n0".into(), "n0.c0".into()), ("n0".into(), "n0.c1".into()),]
        );
    }

    #[test]
    fn compile_is_deterministic_across_calls() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\n  host.facts target=x\n",
        )
        .unwrap();
        let a = compile_program_to_ir_graph(&program).unwrap();
        let b = compile_program_to_ir_graph(&program).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn compile_surfaces_unmapped_verb_with_path() {
        let program = parse_program("space.ritual key=v\n").unwrap();
        let err = compile_program_to_ir_graph(&program).unwrap_err();
        match err {
            CompileError::IrLowering { entry_path, error } => {
                assert_eq!(entry_path, "n0");
                assert!(matches!(error, IrLoweringError::UnmappedVerb { .. }));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn compile_surfaces_missing_arg_with_child_path() {
        let program =
            parse_program("host.verify target=core\n  lab.collect kind=events target=core\n")
                .unwrap();
        let err = compile_program_to_ir_graph(&program).unwrap_err();
        match err {
            CompileError::IrLowering { entry_path, error } => {
                assert_eq!(entry_path, "n0.c0");
                assert!(matches!(error, IrLoweringError::MissingArg { .. }));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    // ------- plan_operational_program -------

    #[test]
    fn happy_path_pipeline_produces_commands_for_every_node() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\nlab.compress kind=drift target=core infer=lab8gb\n",
        )
        .unwrap();
        let manifests = vec![
            manifest_full(),
            infer_manifest("lab8gb", &[PrimitiveName::Compress]),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();

        assert_eq!(plan.graph.nodes.len(), 2);
        assert_eq!(plan.node_plans.len(), 2);
        assert_eq!(plan.node_plans[0].node_id.0, "n0");
        assert_eq!(plan.node_plans[1].node_id.0, "n1");
        // Every node produced a command. We intentionally do not pin the
        // namespace vocabulary here — that is owned by the lowerer and must
        // be free to evolve without breaking the compiler contract.
        assert_eq!(plan.commands().count(), 2);
        for c in plan.commands() {
            assert!(!c.namespace.is_empty());
            assert!(!c.verb.is_empty());
        }
    }

    #[test]
    fn happy_path_pipeline_with_children_lowers_all_levels() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\n  host.inspect target=lab8gb\n",
        )
        .unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        let ids: Vec<_> = plan
            .node_plans
            .iter()
            .map(|p| p.node_id.0.clone())
            .collect();
        assert_eq!(ids, vec!["n0", "n0.c0"]);
        assert_eq!(plan.graph.edges.len(), 1);
    }

    #[test]
    fn blocked_path_pipeline_emits_no_commands_and_returns_closed_error() {
        // Execute is not allowed under PolicyClass::A; policy pass must block.
        let program = parse_program("host.verify target=lab8gb\n").unwrap();
        let ctx_a = AdmissibilityContext {
            policy_class: PolicyClass::A,
            ..ctx()
        };
        let err =
            plan_operational_program(&program, &[manifest_full()], &ctx_a, &MinilabRuntimeLowerer)
                .unwrap_err();
        match err {
            PlanError::ValidationBlocked {
                node_id,
                reason,
                runtime_failure,
            } => {
                assert_eq!(node_id.0, "n0");
                // Reason carries the `policy:` stage prefix from ValidationError.
                assert!(reason.starts_with("policy:"), "reason={reason}");
                // PR 5e: structured failure is tagged at the Policy stage.
                assert_eq!(runtime_failure.stage().as_str(), "policy");
                assert_eq!(runtime_failure.reason_code(), "policy_blocked");
            }
            other => panic!("expected ValidationBlocked, got {other:?}"),
        }
    }

    #[test]
    fn blocked_path_when_manifest_cannot_realize_primitive() {
        // Strip Collect from the manifest; Collect must fail capability pass.
        let mut m = manifest_full();
        m.supported_primitives.remove(&PrimitiveName::Collect);

        let program = parse_program("lab.collect kind=events target=core window=24h\n").unwrap();
        let err =
            plan_operational_program(&program, &[m], &ctx(), &MinilabRuntimeLowerer).unwrap_err();
        match err {
            PlanError::ValidationBlocked {
                node_id,
                reason,
                runtime_failure,
            } => {
                assert_eq!(node_id.0, "n0");
                assert!(reason.starts_with("capability:"), "reason={reason}");
                assert_eq!(runtime_failure.stage().as_str(), "capability");
                assert_eq!(runtime_failure.reason_code(), "capability_unsatisfied");
            }
            other => panic!("expected ValidationBlocked, got {other:?}"),
        }
    }

    #[test]
    fn same_program_produces_same_plan() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\n  host.inspect target=lab8gb\nhost.verify target=core\n",
        )
        .unwrap();
        let a =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        let b =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn idempotency_is_inferred_for_known_primitives() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\nlab.compress kind=drift target=core\nhost.verify target=lab8gb\n",
        )
        .unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();

        // Collect → PureRead
        let n0 = &plan.node_plans[0];
        let c0 = n0
            .idempotency
            .as_ref()
            .expect("collect must have inferred idempotency");
        assert_eq!(c0.class, IdempotencyClass::PureRead);
        assert!(c0.is_wellformed());

        // Compress → DeterministicRecompute
        let n1 = &plan.node_plans[1];
        let c1 = n1
            .idempotency
            .as_ref()
            .expect("compress must have contract");
        assert_eq!(c1.class, IdempotencyClass::DeterministicRecompute);

        // host.verify → no declared contract (honest None, not a guess)
        let n2 = &plan.node_plans[2];
        assert!(
            n2.idempotency.is_none(),
            "host.verify has no declared contract; must surface as None, got {:?}",
            n2.idempotency
        );
    }

    #[test]
    fn idempotency_is_inferred_for_live_execute_verbs() {
        // host.pair: DeduplicatedMaterialAction + ReplayRejection
        let program =
            parse_program("host.pair host_id=h challenge=c agent_pubkey=p correlation_id=r\n")
                .unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        let c = plan.node_plans[0]
            .idempotency
            .as_ref()
            .expect("host.pair contract must be declared");
        assert_eq!(c.class, IdempotencyClass::DeduplicatedMaterialAction);
        assert!(matches!(c.replay_stance, ReplayStance::ReplayRejection));
        assert!(c.is_wellformed());
        assert!(c.key_shape.is_some());

        // outbound.send: DeduplicatedMaterialAction + SilentSuccess
        let program = parse_program(
            "outbound.send contact_id=c campaign_id=a channel=email correlation_id=r\n",
        )
        .unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        let c = plan.node_plans[0]
            .idempotency
            .as_ref()
            .expect("outbound.send contract must be declared");
        assert_eq!(c.class, IdempotencyClass::DeduplicatedMaterialAction);
        assert!(matches!(c.replay_stance, ReplayStance::SilentSuccess));
        assert!(c.is_wellformed());
    }

    #[test]
    fn explicit_lab512_infer_surface_is_preserved_when_capability_exists() {
        let program =
            parse_program("lab.classify kind=failures target=core infer=lab512\n").unwrap();
        let manifests = vec![
            manifest_full(),
            infer_manifest("lab512", &[PrimitiveName::Classify]),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();
        let node = &plan.node_plans[0];
        assert_eq!(node.selected_infer_surface, Some(InferSurface::Lab512));
        assert_eq!(node.selected_substrate_id.as_deref(), Some("lab512"));
        assert_eq!(
            node.command
                .args
                .get("infer_surface")
                .and_then(|v| v.as_str()),
            Some("lab512")
        );
    }

    #[test]
    fn planner_prefers_lab512_for_prioritize_when_available() {
        let program = parse_program("lab.prioritize kind=attention target=core\n").unwrap();
        let manifests = vec![
            manifest_full(),
            infer_manifest("lab512", &[PrimitiveName::Prioritize]),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();
        let node = &plan.node_plans[0];
        assert_eq!(node.selected_infer_surface, Some(InferSurface::Lab512));
        assert_eq!(node.selected_substrate_id.as_deref(), Some("lab512"));
        assert_eq!(
            node.command
                .args
                .get("infer_surface")
                .and_then(|v| v.as_str()),
            Some("lab512")
        );
        match &node.primitive {
            IRPrimitive::Prioritize { infer_surface, .. } => {
                assert_eq!(infer_surface, &InferSurface::Lab512)
            }
            other => panic!("expected routed Prioritize, got {other:?}"),
        }
    }

    #[test]
    fn explicit_lab512_request_blocks_when_capability_is_missing() {
        let program =
            parse_program("lab.classify kind=failures target=core infer=lab512\n").unwrap();
        let err =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap_err();
        match err {
            PlanError::ValidationBlocked {
                node_id,
                reason,
                runtime_failure,
            } => {
                assert_eq!(node_id.0, "n0");
                assert!(
                    reason.contains("requested infer surface lab512"),
                    "unexpected reason: {reason}"
                );
                match runtime_failure {
                    RuntimeFailure::Capability {
                        attempted_substrate,
                        reason_code,
                        ..
                    } => {
                        assert_eq!(attempted_substrate.as_deref(), Some("lab512"));
                        assert_eq!(reason_code, "infer_surface_unavailable");
                    }
                    other => panic!("expected capability failure, got {other:?}"),
                }
            }
            other => panic!("expected ValidationBlocked, got {other:?}"),
        }
    }

    #[test]
    fn lab_summary_prefers_lab512_and_records_selection() {
        let program =
            parse_program("lab.summary kind=drift target=core emit=thread:t42\n").unwrap();
        let manifests = vec![
            manifest_full(),
            exec_infer_manifest("lab512", "lab.summary"),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();
        let node = &plan.node_plans[0];
        assert_eq!(node.selected_infer_surface, Some(InferSurface::Lab512));
        assert_eq!(node.selected_substrate_id.as_deref(), Some("lab512"));
        match &node.primitive {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "lab.summary");
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
            }
            other => panic!("expected Execute bridge, got {other:?}"),
        }
        assert_eq!(node.command.namespace, "lab");
        assert_eq!(node.command.verb, "summary");
        assert_eq!(
            node.command.args.get("emit").and_then(|v| v.as_str()),
            Some("thread:t42")
        );
    }

    #[test]
    fn verify_report_explicit_lab512_blocks_without_capability() {
        let program =
            parse_program("flow.verify_report target=lab8gb infer=lab512 emit=thread:t1\n")
                .unwrap();
        let err =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap_err();
        match err {
            PlanError::ValidationBlocked {
                node_id,
                reason,
                runtime_failure,
            } => {
                assert_eq!(node_id.0, "n0");
                assert!(reason.contains("requested infer surface lab512"));
                match runtime_failure {
                    RuntimeFailure::Capability {
                        attempted_substrate,
                        reason_code,
                        ..
                    } => {
                        assert_eq!(attempted_substrate.as_deref(), Some("lab512"));
                        assert_eq!(reason_code, "infer_surface_unavailable");
                    }
                    other => panic!("expected capability failure, got {other:?}"),
                }
            }
            other => panic!("expected ValidationBlocked, got {other:?}"),
        }
    }

    #[test]
    fn drift_review_prefers_lab512_and_records_selection() {
        let program =
            parse_program("flow.drift_review target=core window=1h emit=thread:t2\n").unwrap();
        let manifests = vec![
            manifest_full(),
            exec_infer_manifest("lab512", "flow.drift_review"),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();
        let node = &plan.node_plans[0];
        assert_eq!(node.selected_infer_surface, Some(InferSurface::Lab512));
        assert_eq!(node.selected_substrate_id.as_deref(), Some("lab512"));
        match &node.primitive {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "flow.drift_review");
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
            }
            other => panic!("expected Execute bridge, got {other:?}"),
        }
    }

    #[test]
    fn recover_cmd_explicit_lab512_is_preserved_when_capability_exists() {
        let program =
            parse_program("flow.recover_cmd id=cmd-1 infer=lab512 emit=thread:t5\n").unwrap();
        let manifests = vec![
            manifest_full(),
            exec_infer_manifest("lab512", "flow.recover_cmd"),
        ];
        let plan =
            plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap();
        let node = &plan.node_plans[0];
        assert_eq!(node.selected_infer_surface, Some(InferSurface::Lab512));
        assert_eq!(node.selected_substrate_id.as_deref(), Some("lab512"));
        assert_eq!(
            node.command.args.get("infer").and_then(|v| v.as_str()),
            Some("lab512")
        );
    }

    #[test]
    fn organize_explicit_lab512_blocks_without_capability() {
        let program =
            parse_program("lab.organize kind=attention target=core infer=lab512\n").unwrap();
        let err =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap_err();
        match err {
            PlanError::ValidationBlocked {
                runtime_failure, ..
            } => match runtime_failure {
                RuntimeFailure::Capability {
                    attempted_substrate,
                    reason_code,
                    ..
                } => {
                    assert_eq!(attempted_substrate.as_deref(), Some("lab512"));
                    assert_eq!(reason_code, "infer_surface_unavailable");
                }
                other => panic!("expected capability failure, got {other:?}"),
            },
            other => panic!("expected ValidationBlocked, got {other:?}"),
        }
    }

    #[test]
    fn plan_artifact_roundtrips_through_json() {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\n  host.inspect target=lab8gb\n",
        )
        .unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        let s = serde_json::to_string(&plan).unwrap();
        let back: CompiledOperationalPlan = serde_json::from_str(&s).unwrap();
        assert_eq!(plan, back);
    }

    #[test]
    fn compile_error_normalizes_bubbles_up_as_planerror() {
        // Build an empty-kind program by hand — the parser would have
        // rejected it, but we can still observe the conversion path by
        // constructing a program with an empty namespace and calling
        // `normalize` via `compile_program_to_ir_graph`. We do this
        // through a tiny AST mutation to exercise the `From` impl.
        use crate::operational_grammar::{ArgValue, OperationalEntry, OperationalLine};
        use std::collections::BTreeMap;

        let mut args = BTreeMap::new();
        args.insert("a".into(), ArgValue::Bare("b".into()));
        let program = OperationalProgram {
            entries: vec![OperationalEntry::leaf(OperationalLine {
                namespace: "".into(),
                verb: "".into(),
                args,
            })],
        };
        let err =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap_err();
        assert!(matches!(
            err,
            PlanError::Compile(CompileError::Normalize(_))
        ));
    }
}
