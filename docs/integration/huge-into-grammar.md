# Huge into Grammar

**Status:** normative · **Type:** ADR / integration memo · **Scope:** constitutional-runtime

Huge is the company domain: **the company as a governed digital entity**, not merely its GTM workflows. Its concerns — Business Canon, Departments, Authority, Claims, Premium Policy, ElasticOperatingConfig, Contacts, Campaigns, Outbound Messages, Inbound Replies, Opt-out — are expressed **inside** the constitutional runtime grammar rather than as a second layer above it.

This memo is not architecture commentary. It is a constitutional decision. If a future change contradicts it without amending this memo, the change is out of order.

Companion memo: [`bundle-into-grammar.md`](./bundle-into-grammar.md) governs the infrastructural domain under the same grammar. The two memos are siblings under the constitution; neither subordinates the other.

---

## 1. Decision

> **Huge is not an application layer above the runtime.** Every huge concern — canon ratification, department binding, authority resolution, claim assertion, premium evaluation, elastic configuration, contact state, campaign execution, outbound send, inbound reply, opt-out — is expressed through the existing sixteen IR primitives, the existing policy classes A/B/C/D, the existing `CapabilityManifest`, the single `evidence_ledger`, and the single lowered `OperationalCommand` queue.

There is no parallel queue. There is no parallel evidence store. There is no "GTM authority" distinct from the constitutional authority model. Business actions are ordinary runtime acts expressed under namespaced kinds (`canon.*`, `department.*`, `claim.*`, `premium.*`, `policy.*`, `elastic.*`, `contact.*`, `campaign.*`, `outbound.*`, `reply.*`, `optout.*`).

**Authority is not inferred.** Department membership does not confer the right to act; canon declares authority and policy evaluation consults it. An actor capable of emitting an action is not, by that fact, authorized to have it executed.

**Claims are constitutional, not feature flags.** A `claim` is a predicate about a contact, account, or world state that gates admissibility of downstream actions. Changing the admissible predicate set is a constitutional act, not a code change hidden in a deployment.

---

## 2. Mapping table

The closed set of huge concepts and how each is expressed **through the existing grammar**. No new primitives are introduced.

| Huge concept | Expression in grammar | Authority boundary | Policy class | Evidence emitted | Notes |
|---|---|---|---|---|---|
| Business Canon | A canon version is a governed, immutable-until-amended artifact. Ratification is a `Confirm` by the canon-signing role over a `Persist`ed canon snapshot. Activation is a subsequent `Persist(canon.active)` referencing the ratified hash. | Founder role per `AuthorityModel` | **C** for ratifying a canon snapshot within the already-admitted amendment process; **D** for changes that alter the amendment process itself, the authority structure, or reserved constitutional semantics | `canon.proposed`, `canon.ratified`, `canon.active` | Canon content itself is data; the act of elevating it is constitutional. |
| Department | Canonical entity declared by canon. Bound into operation via `Persist(department.bound)` against the active `ElasticOperatingConfig`. | Canon (definition) + Elastic (instantiation) | **B** for binding under existing canon | `department.bound`, `department.unbound` | A department has an autonomy class and owned entity kinds; both are canon-declared. |
| Authority model | **Not a primitive.** A constitutional input consulted during `validate_policy` / `validate_admissibility`. Changes to the authority model are amendments to canon. | — | changes classified **D** | `authority.changed` (only when the authority model itself changes) | Authority is *modeled, not inferred*. It is never derived from department membership or capability presence. |
| Claims profile | A profile is a canon-declared set of claim predicates. A single claim about a contact is expressed as `Persist(claim.asserted, { profile_id, claim_kind, subject_ref, predicate, lineage, window, correlation_id })` — one canonical event family, not ad-hoc per-claim event names. Claims gate downstream `Execute` via admissibility, not via procedural checks inside the executor. | Canon (profile definition) + Department (assertion under profile) | **B** for routine assertion; **C** when introducing a new profile | `claim.asserted`, `claim.expired`, `claim.retracted` | A claim without lineage and window is invalid and rejected at persistence. Claim assertion (durable fact) and eligibility evaluation (per-action admissibility decision) are **distinct acts** — see slice notes. |
| Premium policy | A predicate evaluated over a candidate outbound action. Expressed as a `Decide` in the plan graph whose result is `Persist`ed as `premium.evaluated` before `Execute(outbound.*)` is admissible. The evaluation records allowed-reason, bounds, and logging requirements. | Canon (policy text) + Compliance (active bounds) | **B** in steady state; **C** when bounds widen | `premium.evaluated`, `premium.blocked` | Premium is not advisory. Absence of `premium.evaluated` in lineage is a hard admissibility failure. |
| Policy evaluation (general) | Rule matching over a candidate `Execute(outbound.*)` — segment match, hard blocks, throttles. Expressed the same way as premium: `Decide` → `Persist(policy.evaluated)` before execution is admissible. | Compliance | **B** | `policy.evaluated`, `policy.blocked` | Policy and premium are two evaluations in the same lineage; both required. |
| ElasticOperatingConfig | Governed mutable binding under canon. Transitions expressed as `Persist(elastic.bound)` carrying the new snapshot hash and the canon version it satisfies. Branches for simulation are tagged snapshots, not separate entities. | Elastic-authorized roles per canon | **B** for routine; **C** when altering a department autonomy class or allocation envelope | `elastic.proposed`, `elastic.bound`, `elastic.branched`, `elastic.merged` | Violating any canon invariant causes the `Persist` to be rejected at validation. No "live" elastic can contradict canon. |
| Contact | Canonical entity keyed by domain-stable identifier (phone, email, etc.). State transitions via `Persist(contact.*)` and `Classify`/`Compare` for intake. | Sales/SalesOps per canon | **A/B** | `contact.created`, `contact.updated`, `contact.merged`, `contact.suppressed` | Contact identity is constitutional; deduplication is a `Compare` + `Persist(contact.merged)`. |
| Campaign | Canonical entity declared under a department. Audience is a **predicate over contacts**, not a materialized list. Send is `Execute(outbound.send)` per-contact under the campaign's correlation root. | Campaign-owner department | **B** | `campaign.created`, `campaign.scheduled`, `campaign.closed` | Audience is freshly evaluated at lowering of each send. No audience snapshot is authoritative. |
| Outbound message | `Execute(outbound.send, { channel, contact_id, campaign_id, template_id, rendered_payload_ref, … })`. Lowered to an `OperationalCommand` with `target_runtime = provider`. | Campaign-owner department with premium + policy + claims gates | **B** for approved channel; **C** when sending over a newly-admitted channel | `outbound.sent`, `outbound.delivered`, `outbound.failed`, `outbound.bounced` | Channel must be in the canon-declared approved channel set. Unknown channel = admissibility failure, not provider error. |
| Inbound reply | `Observe(reply.inbound, { channel, provider_id })` at ingest, then `Classify(reply.category, …)` producing a category, then downstream actions are ordinary `Execute`s. | SalesOps / Compliance for classification; department routing thereafter | **A** at observation; **B** for any reply-triggered action | `reply.observed`, `reply.classified`, `reply.routed` | The contact conversation view is a projection over rows sharing `contact_id` and correlation lineage, not an authoritative container of truth. |
| Opt-out | `Persist(optout.asserted, { contact_id, channel, source, correlation_id })` with durability class `irreversible`. | Contact (self-assertion via reply classification) or Compliance (administrative) | **A** to assert; **D** to override via superseding state | `optout.asserted`, `optout.scope_widened`, `optout.superseded` | Opt-out is an irreversible assertion that gates `outbound.*` admissibility during lowering — not something the provider decides to refuse. **Revocation is not a symmetric undo.** It does not erase `optout.asserted`; it establishes a separately justified superseding state recorded as `optout.superseded` under class D, with its own lineage. The original assertion remains historically authoritative. |

**Nothing in this table introduces a new primitive.** Every row is `Observe` / `Classify` / `Compare` / `Decide` / `Execute` / `Persist` / `Confirm` — the existing grammar, namespaced.

---

## 3. Invariants

These cannot be violated. Violating one requires amending this memo first (constitutional act).

1. **Canon transitions are constitutional.** Business Canon changes only by `Confirm` of the canon-signing role over a ratified snapshot, producing `canon.ratified` evidence, followed by explicit `Persist(canon.active)`. Silent canon drift is impossible.
2. **No outbound without both gates.** No `Execute(outbound.*)` is admissible unless `premium.evaluated` **and** `policy.evaluated` predicates have been evaluated and recorded in the same lineage **before lowering makes the execution admissible** — not merely before evidence closure. The order between the two is not fixed; the pre-admissibility presence is.
3. **Opt-out is a lowering-time gate.** Any `Execute(outbound.*)` against a contact with an active `optout.asserted` covering the candidate channel is rejected in lowering, not in runtime. No "the provider refused" escape. No re-evaluation at send time substituting for absence at plan time.
4. **Authority is not derived from department membership.** Membership enables participation; authority is canon-declared and consulted by policy evaluation. An actor cannot earn authority by belonging to the right group.
5. **Elastic never contradicts canon.** Any `Persist(elastic.bound)` whose snapshot violates a canon invariant is rejected at validation. No live operating config can contradict the active canon.
6. **Claims require lineage and window.** A `Persist(claim.*)` without explicit evidence lineage and validity window is invalid. Claims are never eternal by default.
7. **Campaign audience is a predicate.** A campaign stores a predicate, not a list. Each candidate send is evaluated fresh. Stored audience snapshots are not admissible as truth.
8. **Channel set is closed until amended.** Outbound channels are enumerated in canon. Sending over a channel outside that set is an admissibility failure, not a feature gap. Adding a channel is classified **C** minimum.
9. **Reply threads are projections.** The conversation view over a contact is derived from evidence sharing `contact_id` and correlation lineage. Message persistence for review is allowed; treating message rows as authoritative truth is not.
10. **Simulation cannot produce real outbound.** When `SimMode ≠ Production`, `Execute(outbound.*)` resolves against mock providers. This is a constitutional requirement of the huge domain — not a convenience of the store layer. (Simulation itself is cross-runtime and not a huge concept; huge *participates* in it, does not define it.)

---

## 4. Normative first slice: `outbound.send` to an eligible contact over an approved channel

This is the slice that proves the memo. It is the smallest operation that exercises the full constitutional chain of the huge domain: contact identity → claims → premium → policy → opt-out → lowering → provider execution → evidence closure → inbound reply observation → reply classification.

**Example instantiation:** SMS send under an SMS campaign. The slice shape is channel-agnostic; SMS is concrete because it has the fewest moving parts and the clearest evidence structure.

### Preconditions

- Active `canon_version` and `elastic_version` exist and are mutually consistent.
- Contact row exists and is not suppressed.
- Campaign row exists under an authorized department and is not closed.
- Channel (SMS) is in the canon-declared approved channel set.
- A `CapabilityManifest` declares `Execute(outbound.send)` for this channel, anchored to a provider implementation hash (or a `sim_stub_id` when `SimMode ≠ Production`).

### IR shape (plan graph, condensed)

```
Decide(eligibility.under_claims, context={ contact_id, campaign_id, required_profile })
   → Persist(eligibility.evaluated)
Decide(premium.admissible, context={ contact_id, campaign_id, channel })
   → Persist(premium.evaluated)
Decide(policy.admissible, context={ contact_id, campaign_id, channel })
   → Persist(policy.evaluated)
Execute(outbound.send, {
  contact_id, campaign_id, channel,
  template_id, rendered_payload_ref,
  correlation_id
})
```

The three `Decide → Persist` pairs populate lineage. `Execute(outbound.send)` becomes admissible only when all three predicates resolve `admissible` in the same lineage and no `optout.asserted` covers the channel for the contact.

**Eligibility ≠ claim assertion.** `eligibility.evaluated` evaluates whether the *already-asserted* claims on the contact satisfy the campaign's required predicate at send time. It does **not** assert new claims. A claim assertion is a separate, durable `Persist(claim.asserted, …)` act, governed by its own authority path. Confusing the two is the single seam most likely to be misused; the evidence kinds are deliberately distinct to prevent it.

### Policy class

**B — Bounded Autonomous** for a campaign over a canon-approved channel with premium/policy/claims predicates recorded.

Escalates to **C** when:
- The channel is newly admitted (first campaign over it after canon amendment).
- Premium evaluation widens the allowed-reason envelope vs. the prior active premium snapshot.
- The target department's autonomy class was changed in the active elastic within the last N events (N canon-declared; deferred).

### Capability

Requires a `CapabilityManifest` entry:

```
{ primitive: Execute, action: "outbound.send", kind: "sms",
  evidence.write: required,
  anchor: payload_bundle_hash(<provider_impl>) | sim_stub_id(<id>) }
```

No anchor → invalid at load time (invariant #10 of the bundle memo; same rule applies here).

### Command shape (lowered)

```
OperationalCommand {
  namespace: "outbound",
  verb: "send",
  target_runtime: "provider",
  args: {
    channel: "sms",
    contact_id, campaign_id, template_id,
    rendered_payload_ref,
    correlation_id,
    canon_version, elastic_version
  }
}
```

`target_runtime = "provider"` per the closed vocabulary defined in the bundle memo. In simulation, the runtime substitutes `target_runtime = "mock"`; the plan graph is unchanged.

### Evidence emitted (success path)

- `eligibility.evaluated` — before Decide-closure; payload `{ contact_id, campaign_id, profile_id, claims_consulted[], result, correlation_id }`. Does **not** mutate claim state.
- `premium.evaluated` — payload `{ contact_id, campaign_id, channel, result, allowed_reason, correlation_id }`.
- `policy.evaluated` — payload `{ contact_id, campaign_id, channel, result, rules_matched, correlation_id }`.
- `outbound.sent` — at provider acknowledgment; payload `{ contact_id, campaign_id, channel, provider_message_id, sent_at, canon_version, elastic_version, correlation_id }`. Idempotency key = `outbound.sent:<provider_message_id>`.
- `outbound.delivered` — if the provider emits delivery confirmation (later, same correlation).

### Evidence emitted (failure path)

- `outbound.failed` — payload `{ contact_id, campaign_id, channel, reason_code, reason_detail, correlation_id }`. Reason codes include: `contact_suppressed`, `channel_not_approved`, `claim_ineligible`, `premium_blocked`, `policy_blocked`, `optout_active`, `capability_missing`, `provider_rejected`, `provider_error`, `version_inconsistent`.

Rejection before lowering emits `policy.blocked` (existing kind) with `{ action: "outbound.send", reason, node_id }`. Rejection at lowering for opt-out emits `outbound.failed` with `optout_active`. For the purposes of this slice, `outbound.failed` includes lowering-time admissibility rejection of an outbound intent, not only provider/runtime execution failure — the operational story remains coherent to the caller.

Failure always closes evidence. Silent drop is not permitted.

### Inbound reply follow-on (same slice, downstream)

When the provider delivers a reply webhook:

- `Observe(reply.inbound, { channel, provider_id })` → `reply.observed` with `{ contact_id, channel, provider_message_id, raw_payload_ref, correlation_id }`.
- `Classify(reply.category, …)` → `reply.classified` with `{ contact_id, category, confidence, correlation_id }`.
- If category is `optout_request`: `Persist(optout.asserted, …)` with source `reply_classified` and durability `irreversible`.
- If category is routable: `Execute(reply.route, …)` under the department authority mapped in canon.

The reply is part of the same slice because it completes the admissibility story: an outbound that produces an opt-out reply must, through the grammar, write an `optout.asserted` that subsequent `Execute(outbound.*)` admissibility will consult.

### Simulation path

Under `SimMode ≠ Production`:
- Plan graph unchanged.
- `target_runtime` substituted to `mock`; provider executor is a stub that produces a synthetic `provider_message_id`.
- Evidence written with `sim_mode` tagged (column exists per migration 005).
- No real send. No real delivery. No real reply — reply ingestion under simulation reads from historical or synthesized payloads.

### Out of scope for this slice

- Threaded multi-turn conversations beyond the immediate send + classify + optout cycle.
- Revenue / KPI attribution closure.
- Claim profile governance (how new profiles are proposed, ratified, retired).
- Elastic branch merge semantics when a simulation branch diverges from the baseline over a campaign run.
- Canon amendment mechanics.

---

## 5. Deferred mappings

Explicitly **not** decided in this memo. Each is a follow-up that must not be silently resolved by implementation choices.

- **Eligibility evaluation semantics.** The canonical separation between durable claim assertion (`claim.asserted`) and per-action admissibility (`eligibility.evaluated`): exact evidence shapes, how eligibility caches or re-evaluates, how it interacts with claim expiry within a correlation.
- **Canon amendment full mechanism.** Proposal → review → quorum → ratification → activation. Who may propose? What is the minimum review window? Does activation atomically supersede the prior canon or is overlap permitted?
- **Claim profile governance.** Introducing a new claim profile, deprecating one, defining backward compatibility for claims asserted under a deprecated profile.
- **Elastic branch merge semantics.** When a simulation branch shows an improved regime, how does it rejoin the baseline? Automatic propose → Compliance ratify → `elastic.merged`? What happens to evidence written under the branch?
- **Conversation thread as projection spec.** Exact SQL view / materialized view definition producing the per-contact thread from `evidence_ledger` rows sharing `contact_id` and correlation lineage. Windowing, redaction, message-body persistence policy.
- **KPI / revenue closure.** How the huge canon core metrics are computed from evidence, how attribution windows are expressed, how counterfactual branches contribute.
- **Channel admission process.** The formal constitutional path to adding a new channel to the approved set: proposal kind, required evidence, ratification class.
- **Premium bound-widening protocol.** When premium evaluation legitimately widens the allowed-reason envelope mid-campaign, what evidence shape authorizes the widening without an amendment?
- **`canon_version` / `elastic_version` columns on `evidence_ledger`.** Should every evidence row carry the active canon and elastic versions at write time for post-hoc analysis? Probably yes; deferred until the release model in the bundle memo is written (the huge side mirrors that decision).

---

## 5a. Landed slices

- **`outbound.send` vertical (2026-04-22).** First constitutional chain landed end-to-end: `eligibility.evaluated → premium.evaluated → policy.evaluated → outbound.admitted → outbound.sent | outbound.failed`, all bound by a single `correlation_id`. Orchestrator in `crates/minilab-store/src/outbound_orchestrator.rs`, proving surface `POST /outbound/send`. Reply ingestion emits `optout.asserted` in parallel with legacy `contact.opted_out`. Integration tests in `crates/minilab-store/tests/outbound_send_slice.rs` prove the §4 success criterion. Deliberate coexistence with the legacy sequence flow (`outreach.sent`, `policy.blocked`, `contact.opted_out`) — no migration in this slice.
- **IR-level `outbound.send` lowering (consolidation pass, 2026-04-22).** `MinilabRuntimeLowerer` now recognizes `ActionKind::Named("outbound.send")` and emits `OperationalCommand { namespace: "outbound", verb: "send", target_runtime: Provider }` with the full success-path evidence contract (`eligibility.evaluated → premium.evaluated → policy.evaluated → outbound.admitted → outbound.sent`). `RuntimeTarget::Provider` is the second pour of the closed bundle-into-grammar §4 vocabulary (after `Platform` from `host.pair`). This closes the formal asymmetry with `host.pair`, which had its lowerer arm from day one.
- **Canonical command dispatch handoff (2026-04-22).** The live path now runs `IR Execute -> MinilabRuntimeLowerer -> OperationalCommand -> dispatch_operational_command -> outbound_orchestrator`, including the proving surface `POST /outbound/send`. The dispatcher is intentionally thin and only reconstructs the typed input for the slice from the lowered command; admissibility and execution semantics remain in `crates/minilab-store/src/outbound_orchestrator.rs`. Integration proof lives in `crates/minilab-store/tests/runtime_dispatch.rs`. See `docs/integration/slice-pattern.md` for the shared anatomy.

---

## 6. Amendment rule

This memo is amended by replacing Section 1, 2, or 3 with an explicit diff and a rationale. Sections 4 and 5 can evolve freely as slices land. Section 6 itself does not change.
