# Bundle into Grammar

**Status:** normative · **Type:** ADR / integration memo · **Scope:** constitutional-runtime

This memo freezes how concepts that originated in the `bundle-minilab` effort (3-Mac cluster, pairing, installs, releases, agent threads) are expressed **inside** the constitutional runtime grammar — IR, policy, capability, evidence, command queue, canon, elastic — rather than alongside it.

It is not architecture commentary. It is a constitutional decision. If a future change contradicts it without amending this memo, the change is out of order.

---

## 1. Decision

> **Bundle is not a separate subsystem.** Every bundle concern — host identity, pairing, install, release, agent command, verify, discovery — is expressed through the same IR primitives, same policy classes, same capability manifest, same evidence ledger, and same lowered `OperationalCommand` queue as the constitutional runtime.

There is no parallel queue. There is no parallel evidence store. There is no parallel "infra" authority. Physical actions are ordinary runtime actions expressed under namespaced kinds.

**Capability is not authority.** A host may be *capable* of an action without being *authorized* to perform it in the active canon + elastic state. Capability declares what can be executed; authority (via canon, elastic, policy) declares what may be executed now. This distinction applies to infra identically to GTM actions.

---

## 2. Mapping table

The closed set of bundle concepts and how each lands in the grammar.

| Bundle concept | Runtime expression | Authority boundary | Policy class | Evidence emitted | Notes / open question |
|---|---|---|---|---|---|
| `host` entity | Four distinct concerns, never collapsed: (1) **canonical entity** — immutable identity, (2) **CapabilityManifest** — what this host can execute, (3) **Elastic binding** — role assignment and operational topology, (4) **Evidence** — observed lifecycle transitions. | Canon (entity existence, role invariants) + Elastic (assignment) + Capability (declaration) | — | `host.registered`, `host.retired`, plus lifecycle kinds (`host.discovered`, `host.paired`, `host.assigned`, `host.activated`, `host.degraded`) | Identity ≠ capability ≠ assignment ≠ state. Conflation is the failure mode this row prevents. |
| `role` (Authors / Hosts / Powers) | `BusinessCanon.host_roles[]` — first-class, not reused `DepartmentSpec`. | Canon (constitutional change to add/remove) | — | `canon.host_role.ratified` | Open: collapse with `DepartmentSpec` later, or keep separate? Default: separate. |
| `assignment_rules` | `ElasticOperatingConfig.host_assignments[]`. Canon sets invariants (e.g. "exactly one Authors"), elastic assigns physical host. | Elastic (flex, branchable) with canon guardrails | — | `elastic.host_assignment.changed` | Precedence: canon invariants win, elastic flexes inside them. Simulation may branch assignments. |
| `pairing_session` + ed25519 challenge | `Execute(host.pair, { host_id, challenge, pubkey })` with pre-auth via `pairing_token_hash`. | Founder / operator (confirmation required unless token pre-authorizes) | **B** (bounded autonomous: template is narrow, parameters are validated) | `host.paired`, `host.pair_failed` | Single atomic act, not a workflow. Replays rejected by challenge nonce. |
| `installation` + desired/applied state | `Execute(install.reconcile, { host_id, installation_id, desired_manifest })` — a Reconcile-shaped act that plans desired/applied convergence and applies one sub-step per divergent payload_service. | Elastic (what to install), Canon (what manifests are permitted) | **B** for routine, **C** for first-install / migrations | `install.reconcile.planned`, `install.reconcile.step.applied`, `install.reconcile.reconciled`, `install.reconcile.failed` | No free-form install. Every install goes through Reconcile + manifest. |
| `agent_command` queue | The lowered `OperationalCommand` queue of the runtime. One queue for outbound SendGrid *and* Mac agent command. | — | inherited from upstream IR node | queue-level, not evidence-level | Namespaces in `verb` disambiguate (`twilio.*`, `agent.*`, `install.*`). |
| `agent_command_lease_events` | Lease lifecycle attached to `OperationalCommand` execution. | Runtime (internal dispatch) | — | `command.leased`, `command.ack`, `command.nack`, `command.timed_out` | Lease is runtime concern, not a separate protocol. |
| `agent_thread` + `agent_message` | A thread is a projection over records sharing correlation lineage; it is never the authoritative container of truth. Message bodies are non-authoritative interaction state — persistence is allowed, treatment as truth is not. | PlaceAgent department authority | A/B depending on triggered action | only actions, outcomes, and closures are evidence — carried by `correlation_id` | Message persistence may exist for replay/review; it does not substitute for evidence. |
| `release` + `manifest_snapshot` | **`ReleaseTuple = (kernel_version, canon_version, elastic_version, capability_manifest_hash, payload_bundle_hashes[])`**. Ratified, signed, published as one row. `ReleaseTuple` is the canonical name; used consistently throughout the memo and code. | Founder if `canon_version` changes, else Compliance/Founder per `AuthorityModel` | **C** when introducing new primitives/actions; **B** when patching payloads only | `release.proposed`, `release.ratified`, `release.activated`, `release.rolled_back` | Release truth is the `ReleaseTuple`, not any single artifact. Artifacts are referenced by hash. |
| `payload` + `payload_service` | Implementation target of a `CapabilityManifest` entry. Capability says "can do X"; payload says "this binary at this hash does X". | — | — | `payload.registered`, `payload.deprecated` | Capabilities never point to free-form binaries. Always to a hashed payload. |
| `verify_steps` + `verify_results` | Evidence-only projection. Each verify step is an `Execute(verify.step, …)` emitting `verify.step.passed` / `verify.step.failed`. | Runtime self-check / install closure | **A** | `verify.step.passed/failed`, `verify.completed` | Verify is part of install closure. No install is `reconciled` without verify evidence. |
| `discovery_config` (mDNS etc.) | `Observe(network.discovery, { protocol, scope })` IR node. | SalesOps/Compliance for cross-site; Runtime for local | **A** (observation) | `discovery.host_found`, `discovery.host_lost` | Discovery is an observation, not a provisioning step. Provisioning uses pair. |
| `secrets_config` | Capability credentials, scoped by capability id. Rotated via `Execute(secret.rotate, …)`. | Compliance | **B** for routine rotation inside an already-authorized envelope; **C** when introducing a new credential class, trust boundary, or authority scope | `secret.registered`, `secret.rotated`, `secret.revoked` | Secrets are never in plaintext in evidence. Only id + envelope hash. |
| `installation_events` (parallel log) | Absorbed into `evidence_ledger`. Namespace: `install.*`. | — | — | see install row above | The separate `installation_events` table in the legacy bundle schema does not migrate; it's a reference shape only. |
| `pairing_events` (parallel log) | Absorbed into `evidence_ledger`. Namespace: `host.*` / `pair.*`. | — | — | see pairing row | Same: reference, not ported. |

---

## 3. Invariants

These cannot be violated. If a proposed change requires violating one, it requires amending this memo first (constitutional act).

1. **No parallel queue.** Every command emitted by the runtime is an `OperationalCommand` in the single lowered queue. "Agent commands" are commands with `agent.*` / `install.*` / `host.*` verb namespaces — not a separate subsystem.
2. **No parallel evidence store.** Every fact produced by the runtime is a row in `evidence_ledger`. No `*_events` shadow tables that hold authoritative history.
3. **No parallel host authority.** `host` entities, their roles, their assignments, their state, and their credentials are owned by Canon + Elastic + CapabilityManifest. Not by an infra subsystem.
4. **No host-touching action without policy evaluation.** Pair, install, reconcile, secret rotation, release activation — all pass validate_structure / validate_policy / validate_capability / validate_admissibility.
5. **No install success without evidence closure.** An install is `reconciled` iff all `install.step.applied` *and* `verify.step.passed` evidence rows are present for that correlation. `FailureToClose` is a real failure.
6. **No release truth outside the `ReleaseTuple`.** If a system claims "running release X", it must point to a single `release.activated` row whose `ReleaseTuple` names the kernel/canon/elastic versions and the payload hashes. No implicit assembly.
7. **No simulation escape.** If `sim_mode ≠ Production`, physical-world executors (pairing keys, real install, real secret rotation) refuse — the same way outbound providers already refuse. Simulation branches run against mock capabilities.
8. **No command without correlation.** Every `OperationalCommand` carries `correlation_id`. Every resulting evidence carries the same `correlation_id`. Threading is trivial because correlation is obligatory.
9. **Capability is not authority.** `CapabilityManifest` declares executability; canon + elastic + policy decide admissibility. An action whose capability is present but whose authority is absent is `policy.blocked`, not executed.
10. **No capability claim without implementation anchor.** Every `CapabilityManifest` entry must reference either (a) a hashed `payload_bundle_hash` naming the implementation, or (b) an explicit simulation stub binding (`sim_stub_id`) valid only outside `SimMode::Production`. Capability without anchor is invalid and rejected at load time.

---

## 4. Normative first slice: `host.pair`

This is the slice that proves the memo. It is the smallest operation that touches identity + capability + policy + evidence + command lowering + physical state, without requiring reconcile or install.

### Preconditions

- `host` row exists (registered via `Execute(host.register)` in a prior correlation).
- `host` is not retired.
- `pairing_token_hash` is present on the host row, **or** an explicit confirmation escalation path is available (see Policy class below).
- Active `canon_version` and `elastic_version` exist and are mutually consistent.

Role assignment is **not** a precondition. Pairing and topology binding are independent lifecycle steps — a host may be paired before it is assigned to a role, and a host may be re-assigned without re-pairing.

### IR shape

```
IrNode {
  id: NodeId("host-pair-<host_id>"),
  body: IRPrimitive::Execute {
    action: ActionKind::Named("host.pair"),
    params: {
      "host_id":        <uuid>,
      "challenge":      <base64 nonce from agent>,
      "agent_pubkey":   <base64 ed25519>,
      "correlation_id": <uuid>,
    }
  }
}
```

`Decide` is not involved. This is a direct `Execute`. If pair needs to be gated behind a higher-level choice, the higher-level node is separate and produces this one after resolution.

### Policy class

**B — Bounded Autonomous.**

Rationale: the template is narrow ("pair this specific host with this specific challenge"), parameters are fully typed and validated, the action is idempotent on nonce (replay-safe), and the authority is pre-delegated via `pairing_token_hash` set by the operator at registration time. If the hash is absent, the node escalates to **C** and requires explicit `Confirm` from Compliance/Founder.

### Capability

Requires capability `host.pair` bound to the runtime executor (not to any Mac agent — the pair action runs *about* the Mac, from the platform side). Capability manifest:

```
{ primitive: Execute, action: "host.pair", evidence.write: required }
```

### Command shape (lowered)

```
OperationalCommand {
  namespace: "host",
  verb:      "pair",
  target_runtime: "platform",
  args: {
    host_id, challenge, agent_pubkey, correlation_id,
    canon_version, elastic_version
  }
}
```

Lowered by a new `HostLowerer` (or added case in the existing lowerer registry). `target_runtime = "platform"` marks this as runtime-side, not agent-side.

**`target_runtime` vocabulary (closed set for v1):**

- `platform` — runs inside the constitutional runtime process itself.
- `agent` — runs inside a host-side agent (Mac daemon).
- `provider` — runs against a registered third-party provider (SendGrid, Twilio, …).
- `mock` — runs against a simulation stub; valid only outside `SimMode::Production`.

Expansion of this set is a constitutional change (amend this memo).

### Evidence emitted (success path)

- `host.pair.initiated` — at lowering time (pre-execution), payload = `{ host_id, challenge_hash, correlation_id }`. Idempotency key = `host.pair.initiated:<host_id>:<challenge_hash>`.
- `host.paired` — on success, payload = `{ host_id, agent_pubkey, pairing_session_id, paired_at, canon_version, elastic_version, correlation_id }`. Idempotency key = `host.paired:<host_id>:<pairing_session_id>`.

Both go to the same `evidence_ledger` as `outreach.sent` and `account.created`.

### Evidence emitted (failure path)

- `host.pair_failed` — payload = `{ host_id, reason_code, reason_detail, correlation_id }`. Reason codes: `invalid_challenge`, `token_mismatch`, `host_not_registered`, `host_retired`, `version_inconsistent`, `policy_block`, `capability_missing`, `replay_detected`, `upstream_error`.

Failure always closes evidence. There is no silent drop.

### Rejection path

Rejected inputs never reach the lowerer. They fail in `validate_policy` or `validate_capability` and emit:

- `policy.blocked` — existing kind, payload carries `{ action: "host.pair", reason, node_id }`.

No `host.pair_failed` is emitted for policy rejection — policy.blocked is already the right kind.

### Simulation path

Under `SimMode::Simulation` or `SimMode::Counterfactual`:

- The action reaches the lowerer.
- The platform executor substitutes a **mock pairer** that validates shape, does not contact any real agent, produces a synthetic `pairing_session_id`.
- Evidence is written with `sim_mode` tagged (column already exists per migration 005).
- No real ed25519 handshake. No real state change on any Mac.

Under `SimMode::Replay`:

- Inputs are replayed from historical evidence. Same lowering, same evidence writes (tagged `replay`), no contact with hosts.

### Out of scope for this slice

- mDNS / `Observe(network.discovery)` — deferred.
- Install pipeline, manifest application, verify steps — deferred.
- Threaded agent conversations — deferred.
- UI surfacing of pair status — `places.minilab.work` already has inspector shapes that can read evidence; wiring is later.
- Migration of any legacy `pairing_events` / `pairing_sessions` data — the legacy bundle DB stays as reference; it is not ported.

---

## 5. Deferred mappings

Explicitly **not** decided in this memo. Each is a follow-up that must not be silently resolved by implementation choices.

- **Release ratification full model.** Who signs? What's the quorum? How is rollback expressed at the tuple level? Does `release.activated` atomically supersede the previous one or is there overlap?
- **`assignment_rules` precedence details.** Canon says "exactly one Authors role must exist". What happens when elastic simulation branches propose zero Authors? Hard reject or soft warning?
- **Install / Reconcile deeper decomposition.** Is each `install.apply_step` one `Execute` child of the Reconcile, or is the whole install a single Reconcile whose lowering internally sequences steps? Affects evidence granularity.
- **Discovery lifecycle.** Expiry of `discovery.host_found` evidence. Does a host found via mDNS auto-register, or does it stay as an observation until `Execute(host.register)` promotes it?
- **Agent thread as projection spec.** What exact SQL view / materialized view definition produces the thread surface from `evidence_ledger`?
- **Capability credential envelope format.** Secret envelope hash — KMS reference, sealed box, or runtime-held?
- **`release_version` column addition to `evidence_ledger`.** Should every evidence row record the active `ReleaseTuple` at write time? Probably yes; deferred until the release model is written.
- **Host lifecycle state machine.** Canonical states and transitions: `discovered → registered → pairable → paired → assigned → active → degraded → retired`. Which transitions are implicit (derived from evidence projections) vs explicit (gated by `Execute`) is deferred.

---

## 5a. Landed slices

- **`RuntimeTarget::Provider` (consolidation pass, 2026-04-22).** Second pour of the closed §4 vocabulary, landed alongside the IR-level `outbound.send` lowerer (see `huge-into-grammar.md` §5a). `Platform` (this slice) and `Provider` (huge) are now both populated; `Agent` and `Mock` remain follow-ups for slices that need them.
- **`host.pair` vertical (2026-04-22).** First constitutional chain landed on the physical-infrastructure axis: `host.pair.initiated → host.paired | host.pair_failed`, all under a single `correlation_id` in `evidence_ledger`. Lowerer in `crates/constitutional-runtime/src/lowering.rs` recognizes `ActionKind::Named("host.pair")` and emits `OperationalCommand { namespace: "host", verb: "pair", target_runtime: Platform }` — first pour of the closed `target_runtime` vocabulary (`agent` / `provider` / `mock` are follow-ups). Orchestrator in `crates/minilab-store/src/host_pair.rs`; proving surface `POST /host-pairings` (act-shaped). Replay is a **deterministic rejection** — `host.pair_failed { reason_code: "replay_detected", phase: "pre_admission" }`, not silent idempotency. Failure payload always carries `phase` (`pre_admission` / `execution`) to distinguish admissibility rejection from executor error. No real platform executor: under non-production `sim_mode` a mock pairer runs; under `SimMode::Production` the orchestrator honestly fails with `upstream_error` at the execution phase (invariant #7). Migration `010_host_pairing_slice.sql` introduces a minimal `host` table (deliberately narrow — no role, no assignment, no capability_manifest, no lifecycle state machine). Integration tests in `crates/minilab-store/tests/host_pair_slice.rs` prove the §4 criterion. Out of scope and deferred: Policy class C escalation to `Confirm`, real ed25519 handshake, CapabilityManifest binding (host-existence is a documented stub stand-in), `host.registered` / `host.retired` / install / discovery / verify / release tuple.
- **Canonical command dispatch handoff (2026-04-22).** The live path now runs `IR Execute -> MinilabRuntimeLowerer -> OperationalCommand -> dispatch_operational_command -> host_pair`, including the act-shaped proving surface `POST /host-pairings`. The dispatcher only verifies the `namespace` / `verb` / `target_runtime` contract and reconstructs the typed slice input; replay, token, version, and execution semantics remain in `crates/minilab-store/src/host_pair.rs`. Integration proof lives in `crates/minilab-store/tests/runtime_dispatch.rs`.

---

## 6. Amendment rule

This memo is amended by replacing Section 1, 2, or 3 with an explicit diff and a rationale. Sections 4 and 5 can evolve freely as slices land. Section 6 itself does not change.
