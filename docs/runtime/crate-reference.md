# Runtime crate reference

**Status:** current for the Phase A consolidation tail · **Crate:** `constitutional-runtime`

This reference maps the public Rust modules to their constitutional roles. It reflects the current flat source layout; Phase B may move files into folders, but the public re-export surface should remain semver-safe.

---

## Feature flags

| Feature | Adds | Notes |
|---|---|---|
| default | Core IR, validation, lowering, planning, execution, file evidence | No external evidence backend is required. |
| `sqlite-evidence` | `SqliteEvidenceStore` | Enables the SQLite evidence store module. |
| `supabase-evidence` | `SupabaseRestEvidenceStore` | Enables the Supabase REST evidence store module. |

Useful checks, when dependencies are available:

```bash
cargo test --workspace
cargo test --workspace --features sqlite-evidence
cargo test --workspace --features supabase-evidence
cargo test --workspace --features "sqlite-evidence,supabase-evidence"
```

---

## Public module map

| Module | Key public types/functions | Responsibility |
|---|---|---|
| `act_identity` | `CanonicalActionId`, `IdentityError` | Typed material-act identity. |
| `capability` | `CapabilityManifest`, `CapabilityBinding`, envelope/guarantee types, `PrimitiveName` | Declares what a runtime or host can realize; does not grant authority by itself. |
| `decision` | `DecideResolver`, `compile_flow`, `compile_node`, `resolve_lower_one`, `lower_compiled_flow`, `assert_decide_free` | Resolves `IRPrimitive::Decide` before normal lowering. |
| `evidence` | `EvidenceContract`, `EvidenceRecord`, `EvidenceStore`, `FileEvidenceStore`, `close_execution_evidence`, `FailureToClose` | Defines required evidence and closes execution only after evidence writes succeed. |
| `evidence_sqlite` | `SqliteEvidenceStore` | Optional SQLite persistence backend behind `sqlite-evidence`. |
| `evidence_supabase` | `SupabaseRestEvidenceStore` | Optional Supabase REST persistence backend behind `supabase-evidence`. |
| `failure` | `RuntimeFailure`, `FailureStage`, `FailurePhase`, `PolicyClassTag` | Structured failure vocabulary shared by planning/execution paths. |
| `idempotency` | `IdempotencyContract`, `IdempotencyClass`, `ReplayStance` | Declares replay semantics inferred for executable nodes. |
| `ingress` | `IngressMode` | Small vocabulary for the L0–L3 ingress ladder; backend wiring is Phase E work. |
| `ir` | `IRPrimitive`, `IrNode`, `ActionKind`, `InferSurface`, `Runtime`-adjacent primitive fields | Canonical semantic graph vocabulary. |
| `lowering` | `Lowerer`, `MinilabRuntimeLowerer`, `LoweringPlan`, `OperationalCommand`, `RuntimeTarget` | Converts admissible IR nodes into the single operational command shape. |
| `operational_grammar` | `parse_line`, `parse_program`, `OperationalProgram`, `OperationalEntry`, parse/lowering errors | Parses operational grammar and converts it into IR; not a second runtime command layer. |
| `plan_executor` | `execute_compiled_plan`, `execute_compiled_plan_async`, `Dispatcher`, `AsyncDispatcher`, `ExecutionReport`, node outcome types | Executes compiled operational plans and halts on first non-success. |
| `planning_compiler` | `compile_program_to_ir_graph`, `plan_operational_program`, `CompiledOperationalPlan`, `IrGraph`, `NodePlan`, `CompileError`, `PlanError` | Builds deterministic IR graphs and execution plans from operational programs. |
| `policy` | `PolicyClass` | Closed constitutional policy classes A/B/C/D. D is reserved at the normal execution boundary. |
| `refs` | `NodeId`, `DataRef`, `TargetRef`, `SurfaceRef`, `PolicyId` | Stable reference wrappers shared by IR, lowering, evidence, planning, validation, and downstream crates. |
| `strong_grammar` | `StrongProgram`, `ReviewSpec`, `PipelineSpec`, `StrongHandler`, `parse_strong_json`, `compile_strong_json_to_ir_graph` | JSON v0 deliberative grammar for Place-Agent review programs; compiles to canonical IR, not to a second command layer. |
| `validation` | `validate_structure`, `validate_policy`, `validate_capability`, `validate_admissibility`, `AdmissibilityContext`, `AdmissibleNode`, `ValidationError` | Enforces structural, policy, capability, and evidence-contract admissibility before lowering/execution. |

---

## Canonical execution path

A material action should follow this path:

1. Parse or construct a candidate act as operational grammar, Strong Grammar, or IR.
2. Resolve any `Decide` node through `decision`.
3. Validate structure, policy, capability, and evidence requirements through `validation`.
4. Lower the admissible node through `MinilabRuntimeLowerer` into an `OperationalCommand` and `LoweringPlan`.
5. Compile deterministic graph/plan metadata through `planning_compiler`.
6. Execute through `plan_executor` using a concrete dispatcher.
7. Close evidence through `close_execution_evidence`; a failed evidence write is `FailureToClose`, not success.

---

## Boundary rules

- `IRPrimitive::Decide` must be gone before `MinilabRuntimeLowerer` sees a node.
- `OperationalCommand` is the only lowered command shape.
- `PolicyClass::D` may be represented for planning/audit but is not executable at the normal runtime boundary.
- `CapabilityManifest` says **can**, not **may**.
- Evidence closure is part of runtime success.
- New primitive/action identity, target runtime, failure stage/phase, or policy vocabulary requires an ADR update before code lands.
