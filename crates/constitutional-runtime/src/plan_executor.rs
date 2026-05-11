//! Deterministic executor for [`CompiledOperationalPlan`].
//!
//! This is the final link of the pipeline at PR 4:
//!
//! ```text
//! CompiledOperationalPlan  ─┐
//!                           ├─▶  execute_compiled_plan  ─▶  ExecutionReport
//! Dispatcher               ─┘
//! ```
//!
//! The executor is intentionally **small**:
//!
//! - iterate `plan.node_plans` in the order produced by the compiler,
//! - dispatch each [`OperationalCommand`] through the caller's
//!   [`Dispatcher`] implementation,
//! - record a [`NodeExecutionResult`] per node,
//! - halt on the first non-[`NodeOutcome::Success`] outcome,
//! - still emit a result for every downstream node so the report is
//!   complete (skipped nodes are [`NodeOutcome::Blocked`] with a reason).
//!
//! It deliberately does **not**:
//!
//! - retry,
//! - branch on `Decide`,
//! - run anything in parallel,
//! - re-evaluate policy or capability (that was PR 3's job),
//! - materialize evidence (that is the dispatcher's / substrate's job).
//!
//! The output [`ExecutionReport`] is serializable, deterministic given a
//! deterministic dispatcher, and carries the original plan alongside the
//! per-node outcomes so diff / replay / simulation tooling can hold a
//! single artifact end-to-end.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::failure::RuntimeFailure;
use crate::idempotency::{IdempotencyContract, ReplayStance};
use crate::lowering::OperationalCommand;
use crate::planning_compiler::{CompiledOperationalPlan, NodePlan};
use crate::refs::NodeId;

// -------------------------------------------------------------------------
// Dispatcher trait
// -------------------------------------------------------------------------

/// Adapter interface between the executor and whatever substrate actually
/// runs a command (HTTP client, tool invocation, in-process handler, mock).
///
/// The executor owns **ordering and halting**; the dispatcher owns
/// **translation and side effects**. The split is deliberate.
pub trait Dispatcher {
    /// Dispatch a single operational command bound to a specific node.
    ///
    /// Implementations must be total — return a [`DispatchOutcome`] for any
    /// command they are asked to run. Panicking is a bug. Implementations
    /// are free to be slow, fallible, or stubbed; the executor treats every
    /// outcome uniformly.
    fn dispatch(&self, node_id: &NodeId, command: &OperationalCommand) -> DispatchOutcome;
}

/// What a single dispatch call reported to the executor.
///
/// Kept intentionally narrow: the executor cares about "advance, halt, or
/// record" — not about substrate-specific error taxonomy. Rich failure
/// normalization lives in [`crate::failure::RuntimeFailure`] and is a
/// concern of dispatchers that want to be constitutionally polite.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DispatchOutcome {
    /// Command executed successfully. `evidence_ref` is an optional opaque
    /// handle the dispatcher can surface (e.g. evidence-ledger row id).
    Success {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evidence_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<Value>,
    },
    /// The dispatcher attempted to execute and the substrate returned a
    /// material failure. `reason_code` is a stable machine-readable tag;
    /// `detail` is optional free-form context.
    ///
    /// `runtime_failure` is the PR 5c addition: the structured, stage-tagged
    /// [`RuntimeFailure`] that the dispatcher can attach when it knows the
    /// constitutional stage (typically `Execution`). Optional for
    /// back-compat with PR 4/5a dispatchers that still speak only in
    /// `reason_code` — downstream consumers prefer the structured form
    /// when present.
    Failure {
        reason_code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_failure: Option<RuntimeFailure>,
    },
    /// The dispatcher refused to attempt the command at all (no route for
    /// this `target_runtime`, unsupported verb, policy pre-flight, …).
    /// Distinct from `Failure` because no material act occurred.
    Blocked { reason: String },
}

impl DispatchOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, DispatchOutcome::Success { .. })
    }
}

// -------------------------------------------------------------------------
// Execution report
// -------------------------------------------------------------------------

/// Per-node outcome in the execution report. Includes the command that was
/// (or would have been) dispatched and whether it actually ran.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub node_id: NodeId,
    pub command: OperationalCommand,
    /// Whether the executor actually called the dispatcher for this node.
    /// False for nodes that were skipped because a prior node halted the run.
    pub dispatched: bool,
    pub outcome: NodeOutcome,
}

/// Closed outcome vocabulary as observed by the executor. Mirrors
/// [`DispatchOutcome`] but also models the "skipped after halt" case that
/// the dispatcher never actually sees.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeOutcome {
    Success {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evidence_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<Value>,
    },
    Failure {
        reason_code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Structured failure companion — see [`DispatchOutcome::Failure`]
        /// for semantics. PR 5c addition.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_failure: Option<RuntimeFailure>,
    },
    Blocked {
        reason: String,
    },
    /// Node was not dispatched because a prior node in the deterministic
    /// order halted the run. The run never gets a chance to know whether
    /// this node would have succeeded.
    Skipped {
        after: NodeId,
    },
}

impl NodeOutcome {
    pub fn is_terminal_halt(&self) -> bool {
        matches!(
            self,
            NodeOutcome::Failure { .. } | NodeOutcome::Blocked { .. }
        )
    }
}

impl From<DispatchOutcome> for NodeOutcome {
    fn from(d: DispatchOutcome) -> Self {
        match d {
            DispatchOutcome::Success {
                evidence_ref,
                detail,
            } => NodeOutcome::Success {
                evidence_ref,
                detail,
            },
            DispatchOutcome::Failure {
                reason_code,
                detail,
                runtime_failure,
            } => NodeOutcome::Failure {
                reason_code,
                detail,
                runtime_failure,
            },
            DispatchOutcome::Blocked { reason } => NodeOutcome::Blocked { reason },
        }
    }
}

/// Full artifact of a single execution pass over a compiled plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionReport {
    /// The plan exactly as it was executed. Carried by value so the report
    /// is self-contained for replay / diff / audit.
    pub plan: CompiledOperationalPlan,
    /// One entry per node in `plan.node_plans`, in the same order.
    pub results: Vec<NodeExecutionResult>,
    /// `Some(id)` if execution halted on this node's non-success outcome.
    /// `None` if every node dispatched successfully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halted_on: Option<NodeId>,
}

impl ExecutionReport {
    pub fn fully_succeeded(&self) -> bool {
        self.halted_on.is_none()
            && self
                .results
                .iter()
                .all(|r| matches!(r.outcome, NodeOutcome::Success { .. }))
    }
}

// -------------------------------------------------------------------------
// Executor
// -------------------------------------------------------------------------

/// Drive a [`CompiledOperationalPlan`] through a [`Dispatcher`] and produce
/// an [`ExecutionReport`].
///
/// Semantics:
///
/// - Iterate `plan.node_plans` in order.
/// - For each node, call `dispatcher.dispatch(&node_id, &command)`.
/// - Map the [`DispatchOutcome`] to a [`NodeOutcome`] and record it.
/// - If the outcome is not `Success`, **halt**: mark `halted_on` and emit
///   `NodeOutcome::Skipped { after: <halting id> }` for every subsequent
///   node without calling the dispatcher.
/// - Return the report.
pub fn execute_compiled_plan<D: Dispatcher + ?Sized>(
    plan: &CompiledOperationalPlan,
    dispatcher: &D,
) -> ExecutionReport {
    let mut results = Vec::with_capacity(plan.node_plans.len());
    let mut halted_on: Option<NodeId> = None;

    for np in &plan.node_plans {
        if let Some(outcome) = already_halted_outcome(&halted_on) {
            results.push(NodeExecutionResult {
                node_id: np.node_id.clone(),
                command: np.command.clone(),
                dispatched: false,
                outcome,
            });
            continue;
        }

        if let Some(outcome) = idempotency_gate(np) {
            let halts = outcome.is_terminal_halt();
            results.push(NodeExecutionResult {
                node_id: np.node_id.clone(),
                command: np.command.clone(),
                dispatched: false,
                outcome,
            });
            if halts {
                halted_on = Some(np.node_id.clone());
            }
            continue;
        }

        let outcome = dispatcher.dispatch(&np.node_id, &np.command);
        let mapped: NodeOutcome = outcome.into();
        let halts = mapped.is_terminal_halt();
        results.push(NodeExecutionResult {
            node_id: np.node_id.clone(),
            command: np.command.clone(),
            dispatched: true,
            outcome: mapped,
        });
        if halts {
            halted_on = Some(np.node_id.clone());
        }
    }

    ExecutionReport {
        plan: plan.clone(),
        results,
        halted_on,
    }
}

/// PR 5d: pre-dispatch idempotency gate.
///
/// If the node declares an idempotency contract whose class forbids
/// mechanical retry AND whose replay stance is `EscalateToConfirm`
/// (i.e. [`crate::idempotency::IdempotencyClass::ConfirmedNonIdempotent`]),
/// the executor **refuses to silently dispatch**. Emits `Blocked` with a
/// closed reason code; the caller is responsible for escalating through a
/// confirmation path before retrying.
///
/// Any other class — including `DeduplicatedMaterialAction` — proceeds to
/// normal dispatch. This gate is about *class C→D escalation*, not about
/// retry policy; retry behavior on transient failure is a separate PR.
fn idempotency_gate(np: &NodePlan) -> Option<NodeOutcome> {
    let contract = np.idempotency.as_ref()?;
    if contract.class.mechanical_retry_allowed() {
        return None;
    }
    if !matches!(contract.replay_stance, ReplayStance::EscalateToConfirm) {
        return None;
    }
    Some(blocked_for_confirmation(contract))
}

fn blocked_for_confirmation(contract: &IdempotencyContract) -> NodeOutcome {
    NodeOutcome::Blocked {
        reason: format!(
            "escalate_to_confirm: idempotency class {} forbids mechanical dispatch",
            contract.class.as_str()
        ),
    }
}

fn already_halted_outcome(halted_on: &Option<NodeId>) -> Option<NodeOutcome> {
    halted_on.as_ref().map(|after| NodeOutcome::Skipped {
        after: after.clone(),
    })
}

// -------------------------------------------------------------------------
// Async executor
// -------------------------------------------------------------------------
//
// The sync [`Dispatcher`] above is perfect for pure / simulation tests and
// for substrates that are already synchronous. Real substrates (HTTP APIs,
// message brokers, databases) are async, so PR 5a pours the same semantics
// over an async trait.
//
// Intentional properties:
//
// - Same [`ExecutionReport`] shape — reports are interchangeable.
// - Same halting discipline — first non-`Success` stops the run and every
//   downstream node is marked [`NodeOutcome::Skipped`].
// - No new crate dependencies — the trait returns a boxed future directly
//   so callers without `async-trait` can still implement it.
//
// Explicitly out of scope (repeated here so the boundary is unambiguous):
// retries, parallelism, branching, retrying on `Skipped`, idempotency
// orchestration. All four belong to a later PR.

use core::future::Future;
use core::pin::Pin;

/// Async analogue of [`Dispatcher`]. Implementations return a `Send` boxed
/// future so the executor can `await` without pulling in `async-trait`.
pub trait AsyncDispatcher {
    fn dispatch_async<'a>(
        &'a self,
        node_id: &'a NodeId,
        command: &'a OperationalCommand,
    ) -> Pin<Box<dyn Future<Output = DispatchOutcome> + Send + 'a>>;
}

/// Async analogue of [`execute_compiled_plan`]. Semantics are identical:
/// deterministic order, halt on first non-`Success`, skip downstream nodes.
pub async fn execute_compiled_plan_async<D: AsyncDispatcher + ?Sized>(
    plan: &CompiledOperationalPlan,
    dispatcher: &D,
) -> ExecutionReport {
    let mut results = Vec::with_capacity(plan.node_plans.len());
    let mut halted_on: Option<NodeId> = None;

    for np in &plan.node_plans {
        if let Some(outcome) = already_halted_outcome(&halted_on) {
            results.push(NodeExecutionResult {
                node_id: np.node_id.clone(),
                command: np.command.clone(),
                dispatched: false,
                outcome,
            });
            continue;
        }

        if let Some(outcome) = idempotency_gate(np) {
            let halts = outcome.is_terminal_halt();
            results.push(NodeExecutionResult {
                node_id: np.node_id.clone(),
                command: np.command.clone(),
                dispatched: false,
                outcome,
            });
            if halts {
                halted_on = Some(np.node_id.clone());
            }
            continue;
        }

        let outcome = dispatcher.dispatch_async(&np.node_id, &np.command).await;
        let mapped: NodeOutcome = outcome.into();
        let halts = mapped.is_terminal_halt();
        results.push(NodeExecutionResult {
            node_id: np.node_id.clone(),
            command: np.command.clone(),
            dispatched: true,
            outcome: mapped,
        });
        if halts {
            halted_on = Some(np.node_id.clone());
        }
    }

    ExecutionReport {
        plan: plan.clone(),
        results,
        halted_on,
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityManifest, PrimitiveName};
    use crate::lowering::MinilabRuntimeLowerer;
    use crate::operational_grammar::parse_program;
    use crate::planning_compiler::plan_operational_program;
    use crate::policy::PolicyClass;
    use crate::validation::AdmissibilityContext;
    use std::cell::RefCell;
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

    fn ctx() -> AdmissibilityContext {
        AdmissibilityContext {
            policy_class: PolicyClass::C,
            runtime_permitted: true,
            at_execution_boundary: true,
            require_evidence_closure: true,
        }
    }

    /// Deterministic recording dispatcher: records every call and returns a
    /// caller-provided script of outcomes. Panics if the script runs out —
    /// that would indicate the executor made an unexpected extra dispatch.
    struct ScriptedDispatcher {
        script: RefCell<Vec<DispatchOutcome>>,
        calls: RefCell<Vec<(NodeId, OperationalCommand)>>,
    }

    impl ScriptedDispatcher {
        fn new(script: Vec<DispatchOutcome>) -> Self {
            Self {
                script: RefCell::new(script),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(NodeId, OperationalCommand)> {
            self.calls.borrow().clone()
        }
    }

    impl Dispatcher for ScriptedDispatcher {
        fn dispatch(&self, node_id: &NodeId, command: &OperationalCommand) -> DispatchOutcome {
            self.calls
                .borrow_mut()
                .push((node_id.clone(), command.clone()));
            let mut s = self.script.borrow_mut();
            assert!(
                !s.is_empty(),
                "dispatcher script exhausted at node {}",
                node_id.0
            );
            s.remove(0)
        }
    }

    fn three_node_plan() -> CompiledOperationalPlan {
        let program = parse_program(
            "lab.collect kind=events target=core window=24h\nlab.compress kind=drift target=core infer=lab8gb\nhost.verify target=lab8gb\n",
        )
        .unwrap();
        let manifests = vec![
            manifest_full(),
            infer_manifest("lab8gb", &[PrimitiveName::Compress]),
        ];
        plan_operational_program(&program, &manifests, &ctx(), &MinilabRuntimeLowerer).unwrap()
    }

    #[test]
    fn all_success_fully_succeeds_and_dispatches_every_node() {
        let plan = three_node_plan();
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: Some("ev-1".into()),
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: Some("ev-2".into()),
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: Some("ev-3".into()),
                detail: None,
            },
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);

        assert!(report.fully_succeeded());
        assert_eq!(report.halted_on, None);
        assert_eq!(report.results.len(), 3);
        for r in &report.results {
            assert!(r.dispatched);
            assert!(matches!(r.outcome, NodeOutcome::Success { .. }));
        }
        // Dispatcher saw nodes in the exact compiler order.
        let ids: Vec<String> = dispatcher.calls().into_iter().map(|(id, _)| id.0).collect();
        assert_eq!(ids, vec!["n0", "n1", "n2"]);
    }

    #[test]
    fn halts_on_first_failure_and_skips_remaining_nodes() {
        let plan = three_node_plan();
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Failure {
                reason_code: "substrate_timeout".into(),
                detail: Some("deadline 5s exceeded".into()),
                runtime_failure: None,
            },
            // intentionally no third entry — if the executor calls again, the
            // ScriptedDispatcher panics.
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);

        assert!(!report.fully_succeeded());
        assert_eq!(report.halted_on, Some(NodeId("n1".into())));
        assert_eq!(report.results.len(), 3);

        assert!(matches!(
            report.results[0].outcome,
            NodeOutcome::Success { .. }
        ));
        assert!(report.results[0].dispatched);

        match &report.results[1].outcome {
            NodeOutcome::Failure { reason_code, .. } => {
                assert_eq!(reason_code, "substrate_timeout");
            }
            other => panic!("expected Failure, got {other:?}"),
        }
        assert!(report.results[1].dispatched);

        match &report.results[2].outcome {
            NodeOutcome::Skipped { after } => assert_eq!(after.0, "n1"),
            other => panic!("expected Skipped, got {other:?}"),
        }
        assert!(!report.results[2].dispatched);

        // Exactly two dispatch calls — the third node was never seen by
        // the dispatcher.
        assert_eq!(dispatcher.calls().len(), 2);
    }

    #[test]
    fn halts_on_first_blocked_like_it_halts_on_failure() {
        let plan = three_node_plan();
        let dispatcher = ScriptedDispatcher::new(vec![DispatchOutcome::Blocked {
            reason: "no route for target_runtime".into(),
        }]);
        let report = execute_compiled_plan(&plan, &dispatcher);

        assert_eq!(report.halted_on, Some(NodeId("n0".into())));
        assert_eq!(report.results.len(), 3);
        assert!(matches!(
            report.results[0].outcome,
            NodeOutcome::Blocked { .. }
        ));
        assert!(report.results[0].dispatched);
        for r in &report.results[1..] {
            assert!(matches!(r.outcome, NodeOutcome::Skipped { .. }));
            assert!(!r.dispatched);
        }
        assert_eq!(dispatcher.calls().len(), 1);
    }

    #[test]
    fn empty_plan_is_a_noop_success() {
        // A plan with zero node_plans should report zero results and no halt.
        let program = parse_program("# only comments\n").unwrap();
        let plan =
            plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
                .unwrap();
        assert!(plan.node_plans.is_empty());

        struct NeverDispatcher;
        impl Dispatcher for NeverDispatcher {
            fn dispatch(&self, _: &NodeId, _: &OperationalCommand) -> DispatchOutcome {
                panic!("dispatcher must not be called for empty plan");
            }
        }

        let report = execute_compiled_plan(&plan, &NeverDispatcher);
        assert!(report.fully_succeeded());
        assert!(report.results.is_empty());
        assert_eq!(report.halted_on, None);
    }

    #[test]
    fn same_plan_same_dispatcher_produces_same_report() {
        let plan = three_node_plan();
        let dispatcher_a = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
        ]);
        let dispatcher_b = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
        ]);
        let a = execute_compiled_plan(&plan, &dispatcher_a);
        let b = execute_compiled_plan(&plan, &dispatcher_b);
        assert_eq!(a, b);
    }

    #[test]
    fn report_roundtrips_through_json() {
        let plan = three_node_plan();
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: Some("e".into()),
                detail: None,
            },
            DispatchOutcome::Failure {
                reason_code: "x".into(),
                detail: Some("y".into()),
                runtime_failure: None,
            },
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);
        let s = serde_json::to_string(&report).unwrap();
        let back: ExecutionReport = serde_json::from_str(&s).unwrap();
        assert_eq!(report, back);
    }

    // -----------------------------------------------------------------
    // PR 5d — idempotency gate
    // -----------------------------------------------------------------

    use crate::idempotency::{IdempotencyClass, IdempotencyContract, ReplayStance};

    fn make_confirmed_non_idempotent_contract() -> IdempotencyContract {
        IdempotencyContract {
            class: IdempotencyClass::ConfirmedNonIdempotent,
            replay_stance: ReplayStance::EscalateToConfirm,
            key_shape: None,
            tags: vec![],
        }
    }

    #[test]
    fn idempotency_gate_blocks_confirmed_non_idempotent_without_dispatching() {
        let mut plan = three_node_plan();
        // Attach a ConfirmedNonIdempotent contract to the middle node.
        plan.node_plans[1].idempotency = Some(make_confirmed_non_idempotent_contract());

        // Script only has two outcomes — if the executor mistakenly
        // dispatches the gated node, the ScriptedDispatcher would panic
        // (exhausted script) on the third call.
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            // (no entry for n1 — it must not reach the dispatcher)
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);

        assert!(report.results[0].dispatched);
        assert!(matches!(
            report.results[0].outcome,
            NodeOutcome::Success { .. }
        ));

        assert!(
            !report.results[1].dispatched,
            "gated node must not dispatch"
        );
        match &report.results[1].outcome {
            NodeOutcome::Blocked { reason } => {
                assert!(
                    reason.starts_with("escalate_to_confirm:"),
                    "reason={reason}"
                );
                assert!(reason.contains("confirmed_non_idempotent"));
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
        assert_eq!(report.halted_on, Some(NodeId("n1".into())));

        // Subsequent node is skipped, as with any halt.
        assert!(!report.results[2].dispatched);
        assert!(matches!(
            report.results[2].outcome,
            NodeOutcome::Skipped { .. }
        ));
        // Dispatcher only saw the first node.
        assert_eq!(dispatcher.calls().len(), 1);
    }

    #[test]
    fn idempotency_gate_allows_deduplicated_material_action() {
        // The live slices attach DeduplicatedMaterialAction contracts; the
        // gate must NOT block them — mechanical retry is allowed.
        let plan = three_node_plan(); // already has outbound.send / host.pair / host.verify
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);
        assert!(report.fully_succeeded());
        assert_eq!(dispatcher.calls().len(), 3);
    }

    #[test]
    fn idempotency_gate_ignores_wellformed_contracts_that_are_not_escalate() {
        // PureRead — should not block.
        let mut plan = three_node_plan();
        plan.node_plans[0].idempotency = Some(IdempotencyContract::pure_read());
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);
        assert!(report.fully_succeeded());
        assert!(report.results[0].dispatched);
    }

    #[test]
    fn results_carry_the_exact_command_that_was_dispatched() {
        let plan = three_node_plan();
        let dispatcher = ScriptedDispatcher::new(vec![
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
            DispatchOutcome::Success {
                evidence_ref: None,
                detail: None,
            },
        ]);
        let report = execute_compiled_plan(&plan, &dispatcher);
        for (plan_entry, result) in plan.node_plans.iter().zip(report.results.iter()) {
            assert_eq!(plan_entry.node_id, result.node_id);
            assert_eq!(plan_entry.command, result.command);
        }
    }
}
