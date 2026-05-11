# Canonical entities and workflows (Supabase-oriented)

Application tables live in your **Supabase** migrations; the repo ships an initial DDL at [`migrations/001_minilab_canonical.sql`](../../migrations/001_minilab_canonical.sql) and Rust serde models in the **`minilab-core`** workspace crate. This document is the **contract** for month-one GTM; align column names and evidence `kind` strings with your runtime and [IR and lowering](../runtime/ir-and-lowering.md).

Evidence rows mirror runtime obligations: append-only `kind` + `payload` jsonb (see `EvidenceRecord` in the crate).

## Entity relationships (month one)

```text
TriggerEvent ──► Account ◄── Contact
                    │           │
                    │           └── opt_out blocks Sequence / sends
                    │
Campaign ──► ClaimsProfile (approved claims)
    │
    ├── MessageFamily (template_ref, claims_allowed[])
    │
    └── Sequence ──► outbound step (account + contact + state)

Reply ──► classifies back to Account / Contact

EvidenceRecord ──► append-only ledger (every material step)

PolicyRule ──► evaluated before sends / content

PremiumExceptionEvent ──► L3 ingress audit trail
```

### Foreign keys (typical)


| Child                                  | Parent                               |
| -------------------------------------- | ------------------------------------ |
| `Contact.account_id`                   | `Account.id`                         |
| `TriggerEvent.account_id`              | `Account.id` (nullable until linked) |
| `Campaign.claims_profile_id`           | `ClaimsProfile.id`                   |
| `MessageFamily.campaign_id`            | `Campaign.id`                        |
| `Sequence.campaign_id`                 | `Campaign.id`                        |
| `Sequence.account_id`                  | `Account.id`                         |
| `Sequence.contact_id`                  | `Contact.id`                         |
| `Reply.account_id`, `Reply.contact_id` | `Account` / `Contact`                |


---

## Entities

### Account


| Field            | Type        | Notes                             |
| ---------------- | ----------- | --------------------------------- |
| `id`             | uuid        | PK                                |
| `name`           | text        | Display                           |
| `domain`         | text        | Normalized; dedupe                |
| `icp_segment`    | text        | Single ICP in month one           |
| `score`          | numeric     | Ranking input                     |
| `stage`          | text        | e.g. new, working, stale, opt_out |
| `strategic_flag` | bool        | Escalation / human review         |
| `dedupe_key`     | text        | Stable merge key                  |
| `metadata`       | jsonb       | Opaque                            |
| `created_at`     | timestamptz |                                   |
| `updated_at`     | timestamptz |                                   |


**Evidence:** `account.created`, `account.merged`, `account.scored`, etc.

### Contact


| Field              | Type        | Notes             |
| ------------------ | ----------- | ----------------- |
| `id`               | uuid        | PK                |
| `account_id`       | uuid        | FK → Account      |
| `email`            | citext      | Dedupe per tenant |
| `phone_e164`       | text        | Optional WhatsApp / messaging target |
| `role_inferred`    | text        | Optional          |
| `buyer_persona_id` | text        | Optional          |
| `opt_out`          | bool        | Hard block sends  |
| `opt_out_at`       | timestamptz | When opted out    |
| `created_at`       | timestamptz |                   |
| `updated_at`       | timestamptz |                   |


Unique: `(tenant_id, email)` if multi-tenant — adjust to your schema.

### TriggerEvent


| Field         | Type        | Notes                   |
| ------------- | ----------- | ----------------------- |
| `id`          | uuid        | PK                      |
| `account_id`  | uuid        | Nullable pre-link       |
| `source`      | text        | Collector id            |
| `signal_type` | text        | Normalized label        |
| `payload`     | jsonb       | Raw + normalized fields |
| `occurred_at` | timestamptz | When signal happened    |
| `created_at`  | timestamptz | Ingestion time          |


### ClaimsProfile

Approved **claims** and bindings for outbound copy. Campaigns reference one profile; message families may further restrict with `claims_allowed`.


| Field        | Type        | Notes                                |
| ------------ | ----------- | ------------------------------------ |
| `id`         | uuid        | PK                                   |
| `name`       | text        | e.g. “Wedge v1 — product X”          |
| `claim_ids`  | text[]      | Subset of global policy library keys |
| `version`    | int         | Bump when claims change              |
| `active`     | bool        |                                      |
| `created_at` | timestamptz |                                      |


### Campaign


| Field               | Type        | Notes                         |
| ------------------- | ----------- | ----------------------------- |
| `id`                | uuid        | PK                            |
| `name`              | text        |                               |
| `channel`           | text        | One channel in month one      |
| `status`            | text        | draft, active, paused, killed |
| `claims_profile_id` | uuid        | FK → ClaimsProfile            |
| `created_at`        | timestamptz |                               |
| `updated_at`        | timestamptz |                               |


### MessageFamily


| Field            | Type        | Notes                             |
| ---------------- | ----------- | --------------------------------- |
| `id`             | uuid        | PK                                |
| `campaign_id`    | uuid        | FK → Campaign                     |
| `label`          | text        | Human-readable variant name       |
| `template_ref`   | text        | Pointer to template / asset id    |
| `claims_allowed` | text[]      | Subset of ClaimsProfile / library |
| `created_at`     | timestamptz |                                   |


### Sequence


| Field          | Type        | Notes                             |
| -------------- | ----------- | --------------------------------- |
| `id`           | uuid        | PK                                |
| `campaign_id`  | uuid        | FK → Campaign                     |
| `account_id`   | uuid        | FK → Account                      |
| `contact_id`   | uuid        | FK → Contact                      |
| `step_index`   | int         | Order in cadence                  |
| `scheduled_at` | timestamptz | When to execute                   |
| `state`        | text        | pending, sent, cancelled, blocked |
| `created_at`   | timestamptz |                                   |
| `updated_at`   | timestamptz |                                   |


### Reply


| Field                      | Type        | Notes                                                                    |
| -------------------------- | ----------- | ------------------------------------------------------------------------ |
| `id`                       | uuid        | PK                                                                       |
| `account_id`               | uuid        | FK → Account                                                             |
| `contact_id`               | uuid        | FK → Contact                                                             |
| `correlation_id`           | uuid        | Links reply back to outbound workflow                                    |
| `provider`                 | text        | e.g. `twilio_sendgrid_parse`, `twilio_whatsapp`                          |
| `channel_message_id`       | text        | Idempotent id from provider                                              |
| `body`                     | text        | Stored for classification                                                |
| `category`                 | text        | positive, objection, referral, unsubscribe, spam_risk, strategic_account |
| `classifier_ingress_level` | int         | 0–3 (matches ingress ladder)                                             |
| `raw_payload`              | jsonb       | Normalized raw provider payload for replay / audit                       |
| `received_at`              | timestamptz |                                                                          |
| `created_at`               | timestamptz |                                                                          |


### EvidenceRecord (business ledger)

Append-only:


| Field            | Type        | Notes                        |
| ---------------- | ----------- | ---------------------------- |
| `id`             | uuid        | PK                           |
| `kind`           | text        | e.g. `outreach.sent`         |
| `payload`        | jsonb       | Correlation ids, ids, hashes |
| `correlation_id` | uuid        | Trace one workflow execution |
| `idempotency_key`| text        | Optional at-most-once key    |
| `sim_mode`       | text        | `production`, `replay`, `simulation`, `counterfactual` |
| `created_at`     | timestamptz |                              |


Optional: `prev_hash`, `row_hash` for tamper-evident chains.

### PolicyRule

Evaluated **before** sends and for content/regional constraints.


| Field        | Type        | Notes                                              |
| ------------ | ----------- | -------------------------------------------------- |
| `id`         | uuid        | PK                                                 |
| `name`       | text        | Human label                                        |
| `scope`      | text        | send                                               |
| `expression` | jsonb       | Serializable predicate (engine-specific)           |
| `priority`   | int         | Higher wins on conflict (define tie-break in code) |
| `active`     | bool        |                                                    |
| `created_at` | timestamptz |                                                    |
| `updated_at` | timestamptz |                                                    |


### PremiumExceptionEvent

Audit trail for **L3** / premium ingress (budget, justification, replay).


| Field             | Type        | Notes                                  |
| ----------------- | ----------- | -------------------------------------- |
| `id`              | uuid        | PK                                     |
| `reason_code`     | text        | Stable taxonomy                        |
| `justification`   | text        | Required human or structured rationale |
| `budget_unit`     | text        | e.g. team, campaign, org               |
| `cost_estimate`   | numeric     | Optional                               |
| `ingress_context` | jsonb       | Model id, latency, input refs          |
| `correlation_id`  | uuid        | Links to workflow / evidence           |
| `created_at`      | timestamptz |                                        |


---

## Week-1 workflows vs evidence kinds


| Step           | Example kinds                        |
| -------------- | ------------------------------------ |
| Signal         | `trigger.ingested`                   |
| Account        | `account.created` / `account.merged` |
| Score          | `account.scored`                     |
| Message family | `message_family.created`             |
| Send           | `outreach.sent`                      |
| Reply receive  | `reply.received`                     |
| Reply          | `reply.classified`                   |
| Block          | `policy.blocked`                     |
| Closure        | correlation-level closure records    |


**Rule:** Opt-out must yield `policy.blocked` and must **not** emit `outreach.sent` for that contact.

---

## IR mapping (conceptual)

- **Observe / Collect / Classify / Prioritize** — discovery, enrichment, ranking (no unbounded outbound without policy).
- **Execute** — bounded sends and side effects; must align with `OperationalCommand` after admissibility.
- **Persist + evidence** — always for material steps.

`Decide` is compiled in the **deliberation** layer (crate `decision` module) before the runtime lowerer sees the graph.

**Related:** [IR and lowering](../runtime/ir-and-lowering.md) · [Documentation index](../README.md) · [Crate reference](../runtime/crate-reference.md)
