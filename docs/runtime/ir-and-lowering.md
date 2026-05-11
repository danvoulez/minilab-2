# IR and lowering

## Sixteen primitives

The IR is a **closed set** of legitimate acts (see `IRPrimitive` in [`../crates/constitutional-runtime/src/ir.rs`](../crates/constitutional-runtime/src/ir.rs)). **Core shapes are not changed casually**; extend behavior via validators, manifests, and lowerers.

**Observation:** `Observe`, `Collect`, `Fetch`  
**Processing:** `Compress`, `Classify`, `Prioritize`, `Compare`  
**Decision:** `Decide`, `Route`, `Schedule`  
**Action:** `Execute`, `Reconcile`, `Emit`  
**Governance:** `Persist`, `Confirm`, `Cancel`  

## Operational grammar

```text
namespace.verb key=value ...
```

**Single representation:** `OperationalCommand` in [`../crates/constitutional-runtime/src/lowering.rs`](../crates/constitutional-runtime/src/lowering.rs) is the only operational command type. The `decision` module does not introduce a parallel shape; it compiles `Decide` away, then calls the same `MinilabRuntimeLowerer`.

`OperationalCommand` uses `BTreeMap` arguments for deterministic rendering (`MinilabRuntimeLowerer::render_pairs`).

## Boundary compiler (`MinilabRuntimeLowerer`)


| Area       | Lowering (illustrative)                                                           |
| ---------- | --------------------------------------------------------------------------------- |
| Observe    | `host.inspect`                                                                    |
| Collect    | `events.collect`                                                                  |
| Fetch      | `events.fetch`                                                                    |
| Compress   | `intel.compress`                                                                  |
| Classify   | `intel.classify`                                                                  |
| Prioritize | `intel.prioritize`                                                                |
| Compare    | `intel.compare`                                                                   |
| Route      | `routing.surface` — **surface + routed primitive name only** (no nested lowering) |
| Schedule   | `runtime.schedule` — **trigger + action primitive name** (no freeform planner)    |
| Confirm    | `checkpoint.await` — **checkpoint object** (role + inner primitive tag + ref)     |
| Persist    | `store.write_intent` — **durability + data ref**, not raw SQL                     |
| Execute    | `namespace.verb` from `ActionKind`                                                |
| Reconcile  | `host.reconcile`                                                                  |
| Emit       | `place.emit`                                                                      |
| Cancel     | `work.cancel`                                                                     |


**`Decide`:** not lowered by `MinilabRuntimeLowerer`; returns `LoweringError::NotLowerableToOperationalGrammar`.

## Decision compilation (`decision`)

Implement `DecideResolver` in [`../crates/constitutional-runtime/src/decision.rs`](../crates/constitutional-runtime/src/decision.rs) (policy / model / human). Then:

- **`compile_node` / `compile_flow`** — replace every `Decide` with concrete primitives (bounded recursion).
- **`assert_decide_free`** — invariant before batch lowering.
- **`resolve_lower_one`** — compile one node, then lower (still yields `OperationalCommand` from `MinilabRuntimeLowerer` only).
- **`lower_compiled_flow`** — lower a slice of already-compiled nodes.

Runtime invariant: **the executive lowerer never sees an unresolved `Decide`.**

## Constitutional admissibility (`validation`)

Three passes (see [`../crates/constitutional-runtime/src/validation.rs`](../crates/constitutional-runtime/src/validation.rs)):

1. **`validate_structure`** — non-empty fields, nesting depth ≤ `MAX_ROUTE_NESTING_DEPTH`, recursive coherence for `Route` / `Schedule` / `Confirm`.
2. **`validate_policy`** — `AdmissibilityContext`: `policy_class`, `runtime_permitted`, `at_execution_boundary` (rejects `Decide` for execution). **A** = autonomous safe (read-only intelligence + emit + bounded route/schedule). **B** = bounded material (all except `Reconcile`). **C** = full primitive set at runtime when capability matches (authority-sensitive). **D** = reserved: **not executable** at `at_execution_boundary: true`; use planning-only validation (`at_execution_boundary: false`) or a non-runtime path.
3. **`validate_capability`** — at least one manifest in the slice must support the primitive, **kind** filter if set, and `evidence.write` if `require_evidence_closure`.

**`validate_admissibility(node, manifests, ctx)`** returns `AdmissibleNode` or `ValidationError`.

## Evidence closure

`LoweringPlan` carries `EvidenceContract`. `EvidenceRecord::from_plan` / `from_execution_result` materialize records. **`close_execution_evidence`** fails with **`FailureToClose`** if any store write fails. Implement `EvidenceStore`: `FileEvidenceStore`, `SqliteEvidenceStore` (`sqlite-evidence`), or `SupabaseRestEvidenceStore` (`supabase-evidence`) for PostgREST-compatible insert.

## Capability manifests

`CapabilityManifest` includes:

- `supported_primitives`
- `supported_kinds` — empty = wildcard; otherwise `kind` on the primitive must match for `Collect`/`Fetch`/…
- `declared_guarantees` — must include `evidence.write` when `AdmissibilityContext.require_evidence_closure` is true

**Related:** [Documentation index](../README.md) · [Constitutional runtime](constitutional-runtime.md) · [Canonical GTM entities](../minilab/canonical-entities.md) · [Crate reference](crate-reference.md)
