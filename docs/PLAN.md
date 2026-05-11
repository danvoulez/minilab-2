# Constitutional Runtime — Organization & Implementation Plan

**Status:** proposed · **Scope:** full workspace (`new-runtime/`) · **Date:** 2026-04-23

This plan has two parts. §1–§2 audit what actually exists and propose the right organization for the current moment. §3–§5 lay out a phased implementation roadmap with explicit anti-goals. It is written in the idiom of the ADRs already in `docs/integration/` — recut reviews, what enters, what is deliberately kept out.

---

## 1. Honest audit of the current state

### 1.1 What is landed and working

| Area | State | Evidence |
|---|---|---|
| Canonical IR with 16 primitives | landed | `crates/constitutional-runtime/src/ir.rs` (181 LOC) |
| Act identity law (ADR 09) | landed | `act_identity.rs` (388), `ActionKind::Canonical(CanonicalActionId)` live |
| Capability manifest + typed binding | landed | `capability.rs` (624), `action_identity: Option<CanonicalActionId>` field |
| Operational grammar surface | landed | `operational_grammar.rs` (1353) — parser, AST, normalizer, line→IR |
| Planning compiler | landed | `planning_compiler.rs` (1269) — determ IDs, IdempotencyContract inference |
| Plan executor (sync + async) | landed | `plan_executor.rs` (783) — halt-on-first-non-success |
| `RuntimeFailure` structured flow | landed | present in `DispatchOutcome::Failure.runtime_failure` |
| Vertical slice — `outbound.send` | landed | `outbound_orchestrator.rs`, 5-station evidence chain |
| Vertical slice — `host.pair` | landed | `host_pair.rs`, 5-station evidence chain |
| Vertical slice — `install.reconcile` | landed | `install_reconcile.rs`, Reconcile-shaped planned/step/reconciled evidence chain |
| Real dispatcher bridging to live slices | landed | `real_dispatcher.rs` (335) |
| Proving surface (act-shaped HTTP) | landed | `minilab-api/src/app.rs` — `POST /outbound/send`, `POST /host-pairings` |
| Evidence ledger (sqlite + supabase) | landed | `evidence_sqlite.rs`, `evidence_supabase.rs` |

### 1.2 What exists as types but is not wired

| Area | State | Note |
|---|---|---|
| `minilab-core::business_canon` (336 LOC) | unused by runtime | Defines company-invariant rules; nothing in `validation.rs` or the slices consults it |
| `minilab-core::elastic_config` (292 LOC) | unused at runtime boundary | Not branched under SimMode; not consulted for admissibility |
| `minilab-core::exploration` (370) | demo path | Wired into `week1_spine.rs` example only |
| `minilab-core::simulation` (35) | stub | Does not integrate with the runtime's SimMode |
| `minilab-core::registry` (74) | stub | No production consumer |
| `IdempotencyContract` in executor | present, not acted on | `NodePlan.idempotency` is populated but no retry/escalate policy consumes it |

These are not defects — they are declared shapes waiting for their constitutional ADR to arrive. The risk is drift: the longer they sit unwired, the more their shapes rot relative to what the runtime actually needs. Phase F below treats this directly.

### 1.3 What is legacy-shaped (pre-slice-pattern substrate)

| Area | LOC | Issue |
|---|---|---|
| `minilab-store::reply` | 1081 | Inbound SendGrid/Twilio ingestion. Large, important, not yet refactored into slice-pattern anatomy. No `reply.received.admitted` marker, no explicit pre-admission chain, evidence closure is ad-hoc |
| `minilab-store::outbound` | 589 | Pre-slice outbound scaffolding. `outbound_orchestrator` is the constitutional path; this file holds helpers that predate it |
| `minilab-store::outreach` | 171 | Legacy sequence-flow authority (per huge-into-grammar §Coexistence) |
| `minilab-store::campaign` | 155 | Campaign state, legacy-shaped |
| `minilab-store::scoring` | 211 | Lead scoring, not governed by the IR |

The huge-into-grammar ADR explicitly sanctions the coexistence of legacy paths during transition. But the plan must name when each legacy path retires.

### 1.4 What is missing entirely

- **Strong Grammar** (Phase 4 of the surgical manual at transcript §5400). No AST, no parser, no compiler. Zero lines.
- **Agent Runtime backend** (Phase 5). No `POST /api/agent-runtime/places/:place_id/messages` route. Zero lines.
- **Runtime-level SimMode branching** — counterfactual scenarios cannot fork state today.
- **Real ed25519 executor for `host.pair`** — production path still uses the mock pairer.
- **Real provider executors for `outbound.send`** — SendGrid/Twilio adapters exist as ingress, not as outbound execution substrate honoring capability envelopes.
- **Observability surface** — no structured metrics, trace spans around evidence close, no operator dashboard.

### 1.5 Module hygiene signals

Large files that will want submodule splits soon (current LOC):

```
operational_grammar.rs  1353   (near ceiling; split parser/normalizer/lowering)
planning_compiler.rs    1269   (split into compile/plan submodules)
reply.rs                1081   (legacy; will be rewritten as slice, not split)
plan_executor.rs         783   (split sync/async + report types)
lowering.rs              743   (target-runtime specific adapters belong elsewhere)
capability.rs            624   (binding/manifest/envelopes split cleanly)
```

Tiny files that suggest orphans or premature splits:

```
ingress.rs         16   (placeholder?)
refs.rs            30   (could fold into ir.rs)
policy.rs          25   (in constitutional-runtime; the real policy lives in minilab-store)
evidence_sqlite    41
simulation.rs      35   (minilab-core)
week1.rs           39   (minilab-core, demo)
```

---

## 2. Organization proposal

### 2.1 Keep the 4-crate shape. Do not split prematurely.

```
constitutional-runtime   kernel: IR, compilers, executor, contracts
minilab-core             business canon + elastic config + exploration
minilab-store            slices + dispatcher + persistence + legacy substrate
minilab-api              HTTP surface
```

This shape is **right for now**. Splitting `constitutional-runtime` into `constitutional-ir` + `constitutional-compilers` + `constitutional-runtime` is tempting given file sizes, but it would re-open module-boundary decisions that are currently stable, and every cross-crate import adds friction to refactors during Phases D–E. Reconsider after Strong Grammar lands and file counts stabilize.

### 2.2 Inside `constitutional-runtime`, consolidate into logical module groupings

Not as separate crates — as `mod` groupings inside `src/`. Proposal:

```
src/
├── ir/                       # types (pure, no deps on policy/evidence)
│   ├── mod.rs                # (was ir.rs)
│   ├── action_identity.rs    # (was act_identity.rs)
│   ├── refs.rs               # (folded in)
│   └── policy.rs             # (folded in — 25 LOC stub)
├── compile/                  # surface → plan
│   ├── mod.rs
│   ├── operational/          # (was operational_grammar.rs, split)
│   │   ├── parser.rs
│   │   ├── ast.rs
│   │   ├── normalizer.rs
│   │   └── to_ir.rs
│   ├── planning.rs           # (was planning_compiler.rs)
│   └── lowering.rs           # kept as-is
├── execute/                  # plan → report
│   ├── mod.rs
│   ├── executor.rs           # (was plan_executor.rs, sync + async core)
│   └── dispatcher.rs         # Dispatcher traits
├── contracts/                # shared vocabulary
│   ├── mod.rs
│   ├── capability.rs
│   ├── failure.rs
│   ├── idempotency.rs
│   └── validation.rs
├── evidence/                 # closure + persistence
│   ├── mod.rs                # (was evidence.rs)
│   ├── sqlite.rs
│   └── supabase.rs
└── lib.rs
```

Benefits: no cross-crate churn, better IDE navigation, clearer ownership per PR. Costs: one rename PR with careful re-export preservation in `lib.rs`.

**Execute this after Phase A, not before.** Doing it mid-Strong-Grammar work adds merge cost.

### 2.3 `minilab-store`: separate slices from legacy substrate

Current `minilab-store/src/` holds three kinds of things, and they are indistinguishable from listing the files:

- **Constitutional slices** (honor slice-pattern): `host_pair.rs`, `outbound_orchestrator.rs`, plus the shared `dispatcher.rs`, `real_dispatcher.rs`
- **Pre-admission evaluators** for outbound: `eligibility.rs`, `premium.rs`, `policy.rs`, `optout_gate.rs`
- **Legacy substrate** (huge-into-grammar §Coexistence): `outbound.rs`, `outreach.rs`, `campaign.rs`, `scoring.rs`, `reply.rs`

Proposal — move into named folders:

```
minilab-store/src/
├── slices/
│   ├── outbound_send/
│   │   ├── mod.rs
│   │   ├── orchestrator.rs
│   │   ├── eligibility.rs
│   │   ├── premium.rs
│   │   ├── policy.rs
│   │   └── optout_gate.rs
│   ├── host_pair/
│   │   └── mod.rs
│   └── reply_received/        # (Phase C — introduced, legacy migrated here)
├── dispatch/
│   ├── mod.rs                 # dispatcher.rs
│   └── real.rs                # real_dispatcher.rs
├── legacy/                    # explicitly named; retirement ADRs attached
│   ├── outbound.rs
│   ├── outreach.rs
│   ├── campaign.rs
│   └── scoring.rs
├── persistence/
│   ├── client.rs
│   ├── store.rs
│   └── webhook.rs
├── evidence.rs
└── lib.rs
```

The `legacy/` folder is the important discipline: it names what coexists during transition (per huge-into-grammar §Coexistence) and makes each retirement an explicit removal rather than silent drift.

### 2.4 `minilab-core`: decide its destiny

`minilab-core` holds 11 files, ~2090 LOC, mostly unused by the live runtime path. Two honest futures:

**Option A — make it the canon authority.** Wire `business_canon` and `elastic_config` into the admissibility chain (Phase F). `minilab-core` becomes the runtime's read-only source for company-invariant rules and operational defaults. `registry.rs`, `departments.rs`, `entities.rs` grow real consumers.

**Option B — retire most of it.** Fold `entities.rs` into `minilab-store`, delete `week1.rs`/`exploration.rs`/`simulation.rs` as demos, and let `business_canon` + `elastic_config` live as future-work memos until someone needs them.

**Recommendation: Option A**, scheduled for Phase F. The types already exist in the shape that matches the huge-bundle doctrine; wiring them is cheaper than rebuilding later and loses nothing if priorities shift.

### 2.5 `places.minilab.work` and `bundle-minilab`

- `places.minilab.work` is the Next.js operator UI. Keep it at workspace root (not under `constitutional-runtime/`) — it has a different toolchain, deploy target, and release cadence. Relationship is consumer-of-`minilab-api`.
- `bundle-minilab/` is reference archive from the infra-into-grammar discovery. Keep it as read-only archive. Do not import from it. Move it to `archive/bundle-minilab/` so it's unambiguously not live code.

---

## 3. Implementation roadmap (8 phases)

Each phase has a recut, a success criterion, and an explicit out-of-scope list matching the discipline of PRs 1–5b.1.

### Phase A — Close the consolidation tail (1–2 days)

**Goal.** Finish cleanup from the PR-1-through-5b.1 arc. Not new capability — hygiene.

**In scope.**
- Write ADR 10 in `docs/integration/`: "What Landed in PRs 1–5b.1" (retrospective of the grammar consolidation).
- Audit orphan modules (`ingress.rs`, `refs.rs`, `policy.rs` in constitutional-runtime). Delete dead code or fold into larger modules.
- Run `cargo clippy --workspace --all-targets -- -D warnings` and fix any that landed silently during the sprint.
- Verify test count is truthful. Expected ~184 after 5b.1 per transcript.
- Update `docs/runtime/crate-reference.md` to reflect the current module layout.

**Out of scope.** Module reorganization (§2.2) — do that right after, as its own PR, not inside this phase.

**Success criterion.** Clippy clean, tests green, ADR 10 landed, zero orphan modules.

---

### Phase B — Module reorganization (§2.2 + §2.3) (2–3 days)

**Goal.** Move files into the module structure proposed in §2.2 and §2.3. No semantic changes.

**In scope.**
- Reorganize `crates/constitutional-runtime/src/` into `ir/`, `compile/`, `execute/`, `contracts/`, `evidence/`.
- Reorganize `crates/minilab-store/src/` into `slices/`, `dispatch/`, `legacy/`, `persistence/`.
- Update `lib.rs` re-exports so external callers are unaffected (semver-safe).
- Update `docs/runtime/crate-reference.md`.

**Out of scope.**
- Splitting `operational_grammar.rs` into submodules — do that when it actually hits 1500 LOC. Today 1353 is fine as a single file.
- Moving anything to a new crate.
- Any behavior change.

**Success criterion.** `cargo test --workspace` identical test count, zero behavior change, all external call sites outside these crates untouched.

---

### Phase C — Third slice: `install.reconcile` (Reconcile-anatomy pioneer) (1 week)

**Status update (2026-05-11).** Landed as the first Reconcile-shaped slice: orchestrator, migration, lowering/dispatch/API surface, integration tests, and `docs/integration/reconcile-anatomy.md`.

**Goal.** Prove the Reconcile-shaped slice anatomy that `docs/integration/slice-pattern.md` §5 explicitly calls out as future work. This is the constitutional reason this phase is before reply-refactor and Strong Grammar: the Reconcile anatomy is the missing proof alongside the admissibility-governed anatomy already proved by `outbound.send` and `host.pair`.

**In scope.**
- Migration `011_install_reconcile_slice.sql` if needed.
- Orchestrator: `submit_install_reconcile(client, input) -> InstallReconcileOutcome`.
- Evidence kinds: `install.reconcile.planned`, `install.reconcile.step.applied`, `install.reconcile.reconciled`, `install.reconcile.failed`.
- Lowerer branch in `constitutional-runtime/src/lowering.rs` for `ActionKind::Canonical("install.reconcile")`.
- Proving surface: `POST /installations/:id/reconcile` in `minilab-api`.
- Integration tests (per slice-pattern §3): happy path, sub-step failure, idempotent re-run, partial convergence.
- New memo: `docs/integration/reconcile-anatomy.md` documenting the Reconcile-shaped variant per slice-pattern §5.
- Update `slice-pattern.md` §5 with the reconcile anatomy now landed.

**Out of scope.**
- Strong Grammar constructs (Phase D).
- Business Canon wiring (Phase F).
- Any UI work.

**Success criterion.** Reconcile slice reconstructs cleanly from the ledger by `correlation_id`, under shared SimMode and failure semantics. Slice-pattern §5 loses its "future work" footnote.

---

### Phase D — Strong Grammar → IR compiler (1.5–2 weeks)

**Status update (2026-05-11).** JSON v0 landed: AST structs, parser, compiler to `IrGraph`, `CompileError` Strong variants, and tests for `SystemReview`, `DriftReview`, `OnSuccess`/`OnFailure` route nodes, `Execute`, and `Confirm`. Textual grammar remains v1.

**Goal.** Phase 4 of the surgical manual. Introduce the deliberative regime. Strong Grammar is the Place Agent's language; it must compile to IR, not speak tools directly.

**In scope (v0, minimal).**
- `crates/constitutional-runtime/src/compile/strong/`
  - `ast.rs` — initial constructs: `Pipeline`, `OnSuccess`, `OnFailure`, `Emit`, `Confirm`, `Execute`, `SystemReview`, `DriftReview`
  - `parser.rs` — JSON-based parse first; grammar-based parse is v1
  - `compiler.rs` — Strong AST → IR graph
- Extension of `CompileError` to include `StrongSyntaxError`, `StrongLoweringError`
- Integration tests: `SystemReview` and `DriftReview` compile to `IrGraph` with expected primitive shapes, `OnSuccess`/`OnFailure` produce correct `Route` nodes

**Out of scope.**
- Natural language ingress (Phase E).
- Surface syntax / textual Strong Grammar beyond JSON.
- Additional constructs beyond the v0 eight.
- Tool registration beyond what's already in the operational verb set.

**Success criterion.** A Strong Grammar program like
```json
{ "SystemReview": {
    "target": "lab8gb",
    "pipeline": ["Collect", "Compress", "Classify", "Prioritize"],
    "on_success": { "Emit": "chat.reply" }
}}
```
compiles to a valid `IrGraph`, passes `validate_admissibility`, produces a `CompiledOperationalPlan` with deterministic IDs, and round-trips through `ExecutionReport` under a scripted dispatcher.

---

### Phase E — Agent Runtime backend (1.5 weeks)

**Status update (2026-05-11).** Existing Agent Runtime and MCP handlers are mounted in `build_app` at `/api/agent-runtime/*` and `/mcp/*`. The first governed NL→candidate→Strong IR→admissibility→plan→scripted execute→evidence pipeline is now landed for advisory, artifact, and proposal-shaped messages; session snapshots expose `runtime_pipeline` with `agent.message.received`, `agent.candidate.classified`, `agent.pipeline.admitted`, node dispatch, and `agent.response.emitted` markers. Remaining Phase E work is replacing the scripted dispatcher with the live `RealDispatcher` path for material candidates and expanding classifier coverage beyond v0 heuristics.

**Goal.** Phase 5 of the surgical manual. Humans speak natural language; the runtime speaks disciplined action. Close the ingress.

**In scope.**
- `POST /api/agent-runtime/places/:place_id/messages` in `minilab-api`.
- Classifier stage: heuristic + LLM hybrid that routes to operational-candidate or strong-candidate.
- Pipeline: NL → structured candidate → IR → validate → plan → execute → evidence.
- Bypass prevention: no agent-runtime output may skip IR, validation, planning, or evidence. Verified by test.
- Evidence kinds: `agent.message.received`, `agent.candidate.classified`, `agent.pipeline.admitted`, `agent.response.emitted`.

**Out of scope.**
- Multi-turn conversation state (v1).
- Place-Agent-specific regimes beyond the single agent (v1).
- Any mutable workflow engine.
- Premium exception handling (that's a Business Canon gate — Phase F).

**Success criterion.** A natural-language message enters the backend, produces a structured candidate, compiles to IR, passes admissibility, executes via `RealDispatcher`, and the resulting `ExecutionReport` plus all intermediate evidence rows are reconstructable by `correlation_id`. Bypass test: no response path emits without IR touching it.

---

### Phase F — Business Canon and Elastic Config wired as runtime authority (1.5 weeks)

**Goal.** Make `minilab-core` real. Today its types exist; nothing consults them. This phase wires them into admissibility.

**In scope.**
- `validate_admissibility` gains a `BusinessCanonContext` parameter that encodes invariant policies, authority model, department responsibilities.
- `ElasticOperatingConfig` becomes the source of runtime envelope defaults (route, allocation, infer/exec surfaces). Loaded once at process start; SimMode branches replace it in-process.
- New evidence kinds: `canon.invariant.evaluated`, `elastic.config.resolved`.
- ADR 11 in `docs/integration/`: "Business Canon as Constitutional Authority" — explicit rules for what goes in canon vs. elastic, matching the surgical-manual §6–§7.
- Migration of wired-but-stubbed honest evaluators (`eligibility`, `premium`) to consult canon instead of hardcoded predicates.

**Out of scope.**
- Retiring the legacy substrate (`outreach.rs`, `campaign.rs`, etc. — that's Phase H).
- Operator UI for editing canon (v2).
- Claims-profile authoring workflows.

**Success criterion.** `submit_outbound_send` consults canon for invariant policies; a config change in `ElasticOperatingConfig` affects live admissibility without code change; evidence rows `canon.invariant.evaluated` are present in the ledger for every live `outbound.send`.

---

### Phase G — `reply.received` as constitutional slice (1 week)

**Goal.** Retrofit the 1081-LOC `reply.rs` into slice-pattern anatomy. The inbound-message ingestion is today the largest non-slice surface in the repo; making it a proper slice closes the asymmetry.

**In scope.**
- New slice in `crates/minilab-store/src/slices/reply_received/`.
- Seven stations per slice-pattern §2: minimal input (verified webhook payload), canonical lowering, dispatch handoff, pre-admission chain (`reply.provider_verified`, `reply.contact_resolved`, `reply.thread_routed`), explicit admitted marker (`reply.received.admitted`), execution boundary (persist + downstream notifications), closed evidence.
- Migration to normalize inbound-reply payloads across Twilio/SendGrid into one shape before admission.
- Legacy `reply.rs` moves to `minilab-store/src/legacy/reply_legacy.rs` with a retirement ADR.
- Integration tests per slice-pattern §3: happy path, signature rejection, unknown-contact path, duplicate-message idempotency.

**Out of scope.**
- New providers beyond Twilio/SendGrid.
- Reply classification / sentiment (that's Phase H observability work or a separate cognitive slice).

**Success criterion.** Every inbound reply goes through the slice pipeline; the legacy ingestion path is feature-flagged to off in production; slice-pattern §4 gains a fourth column documenting the asymmetries this slice introduces.

---

### Phase H — Production hardening and operator surface (2 weeks)

**Goal.** The runtime is correct; make it operable.

**In scope.**
- Real ed25519 executor for `host.pair` (replacing the mock pairer). ADR 12 documents the key-management story.
- Real provider executors for `outbound.send`: SendGrid/Twilio outbound adapters honoring `CapabilityBinding` guarantee/cost/latency envelopes.
- Structured logging via `tracing`, metrics via `opentelemetry`, trace spans around each station of every live slice.
- Retention and compaction for the evidence ledger.
- `places.minilab.work` UI integration: live evidence viewer, per-correlation reconstruction view, SimMode sandbox for operators.
- Retirement of legacy substrate: `outreach.rs`, `campaign.rs`, `scoring.rs`, `outbound.rs` (non-orchestrator helpers) — each with an ADR documenting replacement.

**Out of scope.**
- Multi-tenant isolation beyond existing RLS.
- Horizontal scaling (single-process assumption holds for v1).
- Alternate providers beyond SendGrid/Twilio.

**Success criterion.** The runtime executes a live `outbound.send` against real SendGrid, produces a real `provider_message_id`, and the `places.minilab.work` UI reconstructs the evidence chain from `correlation_id` in under 500ms. Legacy substrate deleted.

---

## 4. Disciplines that hold across all phases

These are not new rules — they're the disciplines the PR-1-through-5b.1 arc already proved. Writing them down makes them enforceable.

1. **ADR before code for constitutional decisions.** New act kinds, new lowerer target types, new failure stages — each requires a dated bullet in the relevant integration memo before the code lands.
2. **Slice pattern adherence.** Every new slice follows slice-pattern.md §2 seven stations, with deliberate asymmetries named in §4. Reconcile-shaped slices (Phase C) follow reconcile-anatomy.md §2 when that memo lands.
3. **No bypass of IR.** Every material act must pass through `IrGraph → validate_admissibility → lower → dispatch`. Ingress points (Phase E) explicitly test this with a bypass-detection test.
4. **Evidence reconstruction.** Every live slice must reconstruct cleanly from the ledger by `correlation_id`. This is a test, not a hope.
5. **Phase-tagged failures.** Every `RuntimeFailure` carries its `FailureStage` and (where applicable) `FailurePhase`. No `reason_code: "error"` untagged.
6. **Idempotency is declared, not inferred at the boundary.** Every `Execute` verb in `operational_grammar::to_ir_primitive` must have its `IdempotencyContract` inferred in `planning_compiler::infer_idempotency_for_primitive`. If it's `None`, that's a slice declaring "not idempotent" on purpose, documented in its ADR.
7. **Closed vocabulary at boundaries.** `FailureStage`, `FailurePhase`, `PolicyClass`, `RuntimeTarget`, `IdempotencyClass`, `ReplayStance` are all closed enums. New members require ADR.

---

## 5. Anti-goals (what NOT to do)

- **Do not put an LLM behind the dispatcher.** The Agent Runtime (Phase E) does NL classification at ingress only. From IR onward, the pipeline is deterministic. An LLM-in-dispatcher would silently bypass admissibility.
- **Do not re-split `constitutional-runtime` into multiple crates before Phase D lands.** File growth is the wrong trigger; crate-level isolation is only worth it when two modules have to evolve on different release cadences. They don't yet.
- **Do not migrate `minilab-core::business_canon` into `constitutional-runtime`.** The layering is intentional: the kernel is substrate-neutral, the canon is company-specific. Folding them blurs a line the spec pays for.
- **Do not introduce new surface kinds without the four declarations** (canonical IR identity, capability binding rule, lowering path, evidence contract) per ADR 09's proibição #3. This applies to Strong Grammar and Agent Runtime ingress equally.
- **Do not expand the operational verb set** during Phases C–E. The subset in `operational_grammar::to_ir_primitive` is closed until a new slice needs a verb (at which point, that slice's ADR extends it).
- **Do not let legacy substrate grow.** Everything in `minilab-store/src/legacy/` (after Phase B) is on a retirement track. New work goes in `slices/`.
- **Do not introduce a workflow engine.** Composition across acts is already expressible via `Schedule`, `Confirm`, `Route` in the IR plus correlation_id chaining. A workflow engine would re-invent what evidence reconstruction already gives us.
- **Do not couple `places.minilab.work` to internal types.** The UI consumes `minilab-api`'s public JSON shape. It does not import any Rust types.

---

## 6. Sequencing summary

| Phase | Duration | Depends on | Ships |
|---|---|---|---|
| A — Consolidation tail | 2 days | PRs 1–5b.1 landed | ADR 10, clippy-clean workspace |
| B — Module reorganization | 3 days | A | New src/ layout, identical behavior |
| C — `install.reconcile` slice | shipped 2026-05-11 | A | Reconcile anatomy proved, third slice |
| D — Strong Grammar compiler | 2 weeks | B | IR graphs from Strong programs |
| E — Agent Runtime backend | 1.5 weeks | D | NL → IR → evidence pipeline |
| F — Business Canon wired | 1.5 weeks | D (for validation surface) | Canon is runtime authority |
| G — `reply.received` slice | 1 week | B, F | Inbound ingestion as slice |
| H — Production hardening | 2 weeks | E, F, G | Real executors, operator UI |

Total: ~11 weeks of focused work to go from the current state (strong grammar-compiled kernel with two live slices) to a production-operable constitutional runtime with five slices, Strong Grammar, NL ingress, canon authority, and a real operator UI.

Shorter paths exist if the priority is "prove more slices fast" (do C then G) versus "prove cognitive depth" (do D then E). The sequencing above balances both.

---

## 7. When to amend this plan

Amend when:
- A phase ships and its scope evolved — update the shipped row to reflect what actually landed.
- A new constitutional question arises that doesn't fit any phase — write its ADR first, then amend the phase that absorbs it.
- A phase's out-of-scope list is violated during implementation — either the violation is an honest scope expansion (update the plan and rereview) or it's scope creep (revert the violation).

This plan is a contract between the work and itself. It evolves freely but deliberately.
