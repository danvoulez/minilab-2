# ADR 10 — What Landed in PRs 1–5b.1

**Status:** accepted · **Type:** ADR / consolidation retrospective · **Scope:** constitutional-runtime, minilab-store, minilab-api · **Date:** 2026-05-11

This ADR closes the consolidation tail after the PR-1-through-5b.1 arc. It records what is now part of the constitutional runtime, what is deliberately still legacy-shaped, and what must not be re-opened without a new ADR.

---

## 1. Decision

The PR-1-through-5b.1 arc is treated as the first closed constitutional-runtime baseline. The baseline is not just code coverage; it is a contract between the IR, capability model, lowering boundary, validation passes, evidence closure, and the first two live vertical slices.

The accepted baseline is:

1. **One canonical IR vocabulary.** Material acts are represented by the closed `IRPrimitive` vocabulary and an `IrNode` carrier. New primitive identity is not introduced casually.
2. **Act identity is typed.** Material actions use `CanonicalActionId` through `ActionKind::Canonical` where identity is known; custom/named paths remain compatibility surfaces, not the preferred constitutional identity.
3. **Capability is separate from authority.** `CapabilityManifest` declares what can be realized; policy, validation, canon, and elastic config decide what may execute.
4. **`Decide` is a compile-time construct.** `MinilabRuntimeLowerer` must not lower unresolved `Decide`; decision compilation resolves it before normal lowering.
5. **Evidence closure is part of success.** A runtime action that cannot write its required evidence is not complete; `FailureToClose` is the explicit failure surface.
6. **Slices are the proving surface.** `outbound.send` and `host.pair` are the landed examples of slice-pattern anatomy, including explicit evidence chains and correlation reconstruction.

---

## 2. Landed runtime surface

The following modules form the accepted runtime core for this baseline:

| Area | Accepted surface | Notes |
|---|---|---|
| Identity | `act_identity` | Typed canonical action identity and validation. |
| IR | `ir`, `refs` | Sixteen primitives plus stable reference wrappers. `refs` remains separate because it is shared by IR, lowering, evidence, planning, and downstream crates. |
| Policy and validation | `policy`, `validation`, `failure` | Policy class D is reserved at the normal execution boundary. Failures carry structured stage/phase vocabulary. |
| Capability | `capability` | Capability manifests bind realizable primitives/actions to execution guarantees. |
| Lowering | `lowering` | The single lowered command shape is `OperationalCommand`; target runtime remains closed vocabulary. |
| Operational grammar | `operational_grammar` | The operational text/AST layer compiles to IR; it is not an alternate command representation. |
| Planning | `planning_compiler`, `idempotency` | Deterministic node IDs and idempotency contracts are inferred before execution. |
| Execution | `plan_executor` | Execution halts on the first non-success result and records structured node outcomes. |
| Evidence | `evidence`, optional backend modules | File evidence is always available; SQLite and Supabase REST stores are feature-gated. |
| Decision boundary | `decision` | `Decide` resolution happens before lowerer entry. |
| Ingress vocabulary | `ingress` | The small `IngressMode` vocabulary is retained as a public module boundary until Phase E gives it a backend consumer. |

---

## 3. Landed slice surface

### 3.1 `outbound.send`

`outbound.send` proves a provider-facing slice with legacy GTM substrate coexistence. Its accepted properties are:

- act-shaped entry through the API and store orchestrator;
- pre-admission gates for eligibility, premium posture, policy, and opt-out;
- explicit evidence for admission, success, and failure;
- deterministic idempotency posture for outbound sends;
- coexistence with legacy outreach/campaign tables while the constitutional path becomes authoritative.

### 3.2 `host.pair`

`host.pair` proves a platform-facing slice for bundle/minilab infrastructure. Its accepted properties are:

- platform target runtime, not provider target runtime;
- replay-sensitive pairing semantics;
- evidence chain around host pairing initiation, success, and failure;
- live dispatch bridge through `RealDispatcher` while the cryptographic executor remains a later production-hardening item.

---

## 4. Module audit from Phase A

Phase A asked whether `ingress.rs`, `refs.rs`, and `policy.rs` were orphan modules. The audit result is:

| Module | Decision | Rationale |
|---|---|---|
| `refs` | Keep as a first-class module. | Its wrappers are used across IR, lowering, evidence, planning, validation, tests, and downstream crates. Folding it into `ir` would increase churn without reducing ambiguity. |
| `policy` | Keep as a first-class module. | `PolicyClass` is constitutional vocabulary used by validation, planning, execution tests, and downstream callers. It deserves its own boundary until the Phase B reorganization moves it under `contracts/`. |
| `ingress` | Keep as a first-class module with explicit docs/tests. | The vocabulary is part of the documented ingress ladder and is intentionally consumed later by Phase E. Keeping the file avoids hiding public vocabulary in `lib.rs`, while the rank helpers and serialization tests make the tiny boundary intentional rather than a placeholder. |

This is intentionally conservative: Phase B will reorganize modules by directory without semantic changes. Phase A should not pre-empt that larger rename PR.

---

## 5. Deferred items

The following are explicitly not part of the closed baseline:

- Strong Grammar compiler and JSON AST surface.
- Agent Runtime backend and natural-language ingress route.
- `install.reconcile` or any third reconcile-shaped slice.
- Business Canon and Elastic Config as runtime admissibility authorities.
- `reply.received` as a constitutional slice.
- Real ed25519 host-pair executor.
- Real SendGrid/Twilio outbound executors that honor capability envelopes.
- Operator UI and metrics/tracing hardening.

Each item remains governed by the phase plan and requires its own ADR or phase-specific memo if its constitutional shape changes.

---

## 6. Invariants carried forward

Future work must preserve these baseline invariants:

1. No material act bypasses IR, validation, planning, lowering, execution, and evidence closure.
2. No second command queue or second evidence authority is introduced for bundle, GTM, or agent-runtime work.
3. No unresolved `Decide` reaches `MinilabRuntimeLowerer`.
4. No new boundary vocabulary member is added without an ADR entry.
5. Legacy substrate may coexist, but new constitutional work goes through slices.
6. Tests must reconstruct live slice behavior from correlation/evidence rather than trusting in-memory success alone.
