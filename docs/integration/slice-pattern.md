# Constitutional Slice Pattern

**Status:** descriptive (extracted from landed work) · **Type:** meta-memo · **Scope:** constitutional-runtime

This memo extracts the **shared anatomy** of the landed vertical slices that proved the constitutional runtime against live domains. It is descriptive, not prescriptive — it names what already worked across the landed slices, so the third slice and beyond can recognize the pattern instead of rediscovering it.

It is not architecture commentary. It is a pattern catalog. If a future slice deviates from this anatomy, the deviation should be deliberate and documented in the slice's own ADR §5a entry.

---

## 1. What the landed slices proved

Three vertical slices have validated the constitutional runtime against live domains:

- **`outbound.send`** (huge-into-grammar §4) — admissibility-governed business action. Proves the grammar can govern a live GTM act with five evidentiary stations under a single correlation.
- **`host.pair`** (bundle-into-grammar §4) — admissibility-governed physical action. Proves the same grammar can govern a live infrastructure act with replay-safe handshake semantics under a single correlation.
- **`install.reconcile`** (bundle-into-grammar §4) — Reconcile-shaped infrastructure action. Proves desired/applied convergence with planned sub-steps, partial convergence, idempotent rerun, and terminal closure under a single correlation.

All three reconstruct cleanly from the ledger by `correlation_id`, under shared simulation and failure semantics.

The runtime is therefore no longer only a memo structure: it has been demonstrated on business acts, physical infra acts, and Reconcile-shaped infra acts, with the same machinery and no special exceptions for either.

---

## 2. Common anatomy

Every constitutional slice landed so far follows the same seven-station shape:

1. **Minimal input.** A small, fully typed payload — never a free-form envelope. Internal IDs (account, dispatch, pairing session) are *resolved or generated* by the orchestrator, never accepted from callers.
2. **Canonical lowering.** The live surface lowers `ActionKind::Named("<namespace>.<verb>")` through `MinilabRuntimeLowerer` into one `OperationalCommand`, with an explicit success-path evidence contract and the right `RuntimeTarget`.
3. **Dispatch handoff.** `dispatch_operational_command` checks the `namespace` / `verb` / `target_runtime` contract and reconstructs the minimal typed input for the slice orchestrator. It does not perform policy or execution work itself.
4. **Pre-admission chain.** A linear sequence of evaluators that each emit one `*.evaluated` (or equivalent) evidence row with a structured payload. Failures here close evidence with `phase: pre_admission`.
5. **Explicit admitted marker.** A single dedicated kind (`outbound.admitted`, `host.pair.initiated`) that names the moment the act crosses from admissibility into execution. This marker is constitutional, not a debug breadcrumb: it lets the ledger distinguish "allowed" from "performed".
6. **Execution boundary.** The orchestrator delegates to an executor (provider HTTP call, mock pairer, etc.) under a `sim_mode` gate that respects the no-simulation-escape invariant. Real-world executors refuse outside Production; missing real executors fail honestly with `upstream_error` at `phase: execution`.
7. **Closed evidence + correlation reconstruction.** Either success (`outbound.sent`, `host.paired`) or failure (`outbound.failed`, `host.pair_failed`). Failure payloads always carry a `reason_code` plus a `phase` field. There is no silent drop. Idempotency keys are assigned per kind so the ledger's own unique-key protection is a safety net under the semantic outcome — not a substitute for it. Every row in the chain carries the same `correlation_id`, so reading the ledger filtered by that single identifier reproduces the entire constitutional lineage of the act, in order, without code interpretation.

Each station is also `sim_mode`-tagged at the ledger row level (column written by `insert_ledger_row`), so simulation is a property of the same runtime, not a parallel environment.

---

## 3. Reusable slice template

When opening the next slice, the following template captures the proven shape:

| Station | Concrete artifact |
|---|---|
| Evidence kinds | `*.evaluated` per pre-admission gate, `*.admitted` (or `*.initiated`), `*.sent` (or `*.<succeeded>`), `*.failed` |
| Dispatcher handoff | `crates/minilab-store/src/dispatcher.rs`, with one match arm per landed constitutional act and explicit contract errors for anything else |
| Pre-admission evaluators | One module per gate under `crates/minilab-store/src/`, returning a structured `Outcome` enum that the orchestrator inspects |
| Orchestrator | `crates/minilab-store/src/<slice>.rs` exposing `submit_<slice>(client, input)` returning `<Slice>Outcome::{Sent|Paired|… , Rejected|Failed}` |
| Lowerer branch | Specific match arm in `crates/constitutional-runtime/src/lowering.rs` for `ActionKind::Named("<namespace>.<verb>")`, declaring the success-path evidence contract and the proper `RuntimeTarget` |
| Proving surface | Act-shaped HTTP route (e.g. `POST /<plural-noun>` for state-creating acts, or `POST /<resource>/actions/<verb>` if a parent resource is essential), mounted in `crates/minilab-api/src/app.rs` |
| Integration tests | `crates/minilab-store/tests/<slice>_slice.rs` with at least: happy path, one pre-admission rejection, one execution failure, and one slice-specific seam (e.g. replay, opt-out, idempotency) |
| ADR §5a entry | Single dated bullet in the relevant integration memo, naming files, intentional asymmetries with prior slices, and explicit deferred items |

The template is a guide, not a constitution. Slices may legitimately omit stations they do not need (host.pair has no pre-admission policy.evaluated row because there is no policy class B/C escalation in this slice).

---

## 4. Intentional asymmetries between landed slices

These differences are **deliberate** and should not be "fixed" by future passes without re-opening the relevant ADR. They reflect honest differences between the two domains, not inconsistency in the runtime.

| Concern | `outbound.send` | `host.pair` | Why the asymmetry exists |
|---|---|---|---|
| IR lowerer | Initially deferred; landed in consolidation pass alongside this memo | Landed in original slice | host.pair touched the lowerer anyway to introduce `RuntimeTarget::Platform`; outbound caught up once the pattern was stable |
| Replay semantics | Idempotent (provider-side) — second send with same idempotency key is a no-op success | Deterministic rejection — second pair with same challenge writes `host.pair_failed { replay_detected }` | Sends are operationally idempotent by design; pairings are security handshakes where replay is itself the event of interest |
| Coexistence with legacy | Deliberate — `outreach.sent`, `policy.blocked`, `contact.opted_out` remain authoritative for the existing sequence flow during transition | None — `host` table created fresh by this slice, no legacy schema to coexist with | Huge has a live legacy substrate (sequences, outreach jobs); bundle does not |
| Substrate | Reads existing `contact`, `account`, `campaign`, `policy_rule`, `message_family` tables | Required a new minimal `host` table (migration `010_host_pairing_slice.sql`) | Huge's substrate predates the slice; bundle's did not exist |
| Pre-admission depth | Three evaluators (`eligibility.evaluated`, `premium.evaluated`, `policy.evaluated`) plus an opt-out gate that is semantically separate from policy | One evaluator family in-orchestrator (registration / retirement / token / version / replay), no separate `*.evaluated` rows | Huge needs claim-vs-admissibility seam (huge memo invariant); bundle's pre-admission is narrower and atomic |
| `target_runtime` | `Provider` (third-party SendGrid/Twilio executor) | `Platform` (executor lives inside the runtime process) | The closed ADR vocabulary; each slice uses the right member |
| Reconcile shape | Not applicable | Not applicable | `install.reconcile` uses `planned → step.applied* → reconciled/failed`, documented in `reconcile-anatomy.md`; it is related but not forced into the single-act admitted/sent shape |
| Honest stubs | `eligibility` and `premium` evaluators have full evidence shape, minimal predicate content | Capability check is a documented stub (host existence stands in) | Both slices made the **same kind** of trade-off — name the shape, defer the content — but at different stations |

These rows are not a backlog. They are the truthful catalog of what each domain needed.

---

## 5. What the pattern excludes

The slice template is for acts that are **single, named, and admissibility-governed**. It is not the right shape for:

- **Long-running flows** that span multiple acts (e.g. a sequence of three sends with a wait window between each). Those compose multiple slice acts, each with its own correlation.
- **Reconcile-shaped operations** that converge desired and applied state over multiple sub-executes. The first landed example is `install.reconcile`; its specific anatomy is captured in `docs/integration/reconcile-anatomy.md` with `*.planned`, `*.step.applied`, `*.reconciled`, and `*.failed` closure.
- **Pure observations** (`Observe`, `Collect`) that produce evidence but are not acts. Those use the intelligence-loop primitives directly without the admission/execution split.

The slice pattern is one canonical anatomy among several. Naming it explicitly prevents future slices from being miscategorized into it.

---

## 6. When to amend this memo

Amend this memo when:

- a third slice lands that follows the same anatomy → add it to §1 and update §4 if it introduces a fourth axis of asymmetry;
- a slice deliberately deviates from the anatomy → add a row in §4 explaining the deviation;
- a station in §2 turns out to be optional in practice → mark it optional and cite the slice that demonstrated this.

This memo evolves freely as slices land. It does not require constitutional ratification because it describes (not prescribes) — the prescriptive work lives in the per-domain ADRs.
