//! Constitutional runtime core types for Minilab-style systems.
//!
//! Execution is not sovereign: material actions must be semantically admissible,
//! policy-permitted, capability-realizable, and evidentially accountable.
//!
//! See `docs/runtime/constitutional-runtime.md` for the full definition.

pub mod act_identity;
pub mod capability;
pub mod decision;
pub mod evidence;
#[cfg(feature = "sqlite-evidence")]
pub mod evidence_sqlite;
#[cfg(feature = "supabase-evidence")]
pub mod evidence_supabase;
pub mod failure;
pub mod idempotency;
pub mod ingress;
pub mod ir;
pub mod lowering;
pub mod operational_grammar;
pub mod plan_executor;
pub mod planning_compiler;
pub mod policy;
pub mod refs;
pub mod strong_grammar;
pub mod validation;

pub use act_identity::{CanonicalActionId, IdentityError};
pub use capability::{
    primitive_kind, CapabilityBinding, CapabilityManifest, CostEnvelope, EvidenceGuarantee,
    GuaranteeEnvelope, LatencyEnvelope, PrimitiveName,
};
pub use decision::{
    assert_decide_free, compile_flow, compile_node, contains_decide, lower_compiled_flow,
    materialize_primitive, resolve_lower_one, DecideResolver, PlannerError, PlannerLoweringError,
};
pub use evidence::{
    close_execution_evidence, EvidenceContract, EvidenceRecord, EvidenceStore, EvidenceStoreError,
    FailureToClose, FileEvidenceStore,
};
pub use failure::{FailurePhase, FailureStage, PolicyClassTag, RuntimeFailure};
pub use idempotency::{IdempotencyClass, IdempotencyContract, ReplayStance};
pub use ingress::IngressMode;
pub use ir::{
    ActionKind, DurabilityClass, IRPrimitive, InferSurface, IrNode, Kind, ReconcileMode, Role,
    Schema, Trigger, Window,
};
pub use lowering::{
    Lowerer, LoweringError, LoweringPlan, MinilabRuntimeLowerer, OperationalCommand, RuntimeTarget,
};
pub use operational_grammar::{
    parse_line, parse_program, ArgValue, IrLoweringError, OperationalEntry, OperationalLine,
    OperationalProgram, ParseError, ParseErrorKind,
};
pub use plan_executor::{
    execute_compiled_plan, execute_compiled_plan_async, AsyncDispatcher, DispatchOutcome,
    Dispatcher, ExecutionReport, NodeExecutionResult, NodeOutcome,
};
pub use planning_compiler::{
    compile_program_to_ir_graph, plan_ir_graph, plan_operational_program, CompileError,
    CompiledOperationalPlan, Edge, IrGraph, NodePlan, PlanError,
};
pub use policy::PolicyClass;
pub use refs::{DataRef, NodeId, PolicyId, SurfaceRef, TargetRef};
pub use strong_grammar::{
    compile_strong_json_to_ir_graph, compile_strong_program, parse_strong_json, ConfirmSpec,
    ExecuteSpec, PipelineSpec, PipelineStep, ReviewSpec, StrongHandler, StrongProgram,
};
pub use validation::{
    check_capability, validate_admissibility, validate_capability, validate_policy,
    validate_structure, AdmissibilityContext, AdmissibleNode, ValidationError,
    MAX_ROUTE_NESTING_DEPTH,
};

#[cfg(feature = "sqlite-evidence")]
pub use evidence_sqlite::SqliteEvidenceStore;
#[cfg(feature = "supabase-evidence")]
pub use evidence_supabase::SupabaseRestEvidenceStore;
