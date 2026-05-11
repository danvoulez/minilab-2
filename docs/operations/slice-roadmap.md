# Minilab slice roadmap — from now to “the end”

This is a **vertical-slice** plan: each slice ships a **thin end-to-end path** that closes with **evidence** (or fails closed), then widens. It aligns with [30-day rollout](gtm-30-day-rollout.md), [canonical entities](../minilab/canonical-entities.md), and [repository layout and split criteria](../repo/layout-and-split-criteria.md).

**Living plan:** When strategy or external constraints change, **edit slices and ordering** to match and record each update in the [plan revision log](#plan-revision-log) below.

---

## What we mean by “the end”

**End state (v1)** — *good enough to call the wedge real:*

- One ICP, one persona, one offer, **one outbound channel**, one booking path (unchanged from month-one constraints).
- **Continuous** operation: queue produced daily without manual copy-paste for the happy path.
- **Every material step** has an append-only evidence row (`kind` + payload + correlation).
- **Opt-out / stop rules** hard-block sends; never emit `outreach.sent` when blocked.
- **Ingress ladder** in use: L0/L1/L2 routine; **L3 premium** rare, logged, justified (`PremiumExceptionEvent`-class audit).
- **Explainability**: for sampled touches, you can answer why contacted and why that message variant (policy + pointers into evidence).
- **KPIs visible**: autonomy rate, evidence closure rate, positive reply rate, premium escalation trending toward ~0.

That is **“the end” of the constitutional GTM wedge** — not the end of all product ambition.

**Beyond v1 (explicitly later):** second ICP/channel, full Minilab **distributed** stack (Places, hosts, enrollment) as in [Minilab architecture](../minilab/architecture.md), or splitting repos per [repository layout and split criteria](../repo/layout-and-split-criteria.md). Those are **separate horizons**; slices below stop at v1 unless noted.

---

## Rules for every slice

1. **Vertical:** touch domain + persistence + (when relevant) constitutional boundary + evidence.
2. **Done = observable:** demo, test, or dashboard — not “module exists.”
3. **Change `constitutional-runtime` only** when a slice proves a gap (semantics, admissibility, lowering, evidence) — see [manifesto](../minilab/manifesto.md) discipline.
4. **Business judgment** — when a slice encodes ICP, signals, claims, opt-out, policy, regions, or premium, capture the decisions that affect admissibility or evidence (policy, config, or ticket notes), not silent defaults in code alone.

---

## Slices (ordered)

### Slice 0 — **Foundation** *(current repo baseline)*

| | |
| --- | --- |
| **Ships** | `constitutional-runtime`; `minilab-core` types; `migrations/001_minilab_canonical.sql`; examples `week1_spine`, `constitutional_touch`. |
| **Exit** | `cargo test --workspace` green; docs describe IR + entities + rollout. |
| **Notes** | No live DB or ESP required. |

---

### Slice 1 — **Durable spine (read/write one correlation)**

| | |
| --- | --- |
| **Goal** | One **correlation id** flows through **your** database (Supabase or local Postgres): create rows, read back. |
| **Ships** | Applied migration; minimal client (SQL, Supabase client, or thin Rust service) that inserts `account` + `contact` + `evidence_ledger` rows; idempotent keys where needed. |
| **Exit** | Script or job creates a wedge account and writes ≥2 evidence kinds for that correlation; replay by `correlation_id`. |
| **Depends** | S0. |
| **Notes** | Verify Postgres and hosting behavior against official docs when version or environment matters. |

---

### Slice 2 — **Signal → account (ingest path)**

| | |
| --- | --- |
| **Goal** | **TriggerEvent** ingestion produces `trigger.ingested` and `account.created` (or merge) evidence. |
| **Ships** | One collector input (webhook, CSV, or manual CLI); normalization + dedupe key; link or create `account`. |
| **Exit** | End-to-end run from raw signal to ledger rows; no outbound yet. |
| **Depends** | S1. |

---

### Slice 3 — **Score + message family (still no external send)**

| | |
| --- | --- |
| **Goal** | Rank accounts and attach an approved **message family** under a **campaign** + **claims profile**. |
| **Ships** | Scoring job (rule-based or model — L1 OK); `ClaimsProfile` / `Campaign` / `MessageFamily` rows; evidence `account.scored`, `message_family.created`. |
| **Exit** | At least one account has score + family; evidence complete for that path. |
| **Depends** | S2. |

---

### Slice 4 — **Policy + opt-out gate (mock send)**

| | |
| --- | --- |
| **Goal** | **Before** any send: evaluate `PolicyRule` (minimal: scope `send` + opt-out); emit `policy.blocked` or proceed to mock send. |
| **Ships** | `Sequence` row `pending` → `blocked` or `sent` (mock); **no** real ESP. |
| **Exit** | Opt-out contact never gets `outreach.sent`; allowed contact gets mock `outreach.sent` + payload in evidence. |
| **Depends** | S3. |

---

### Slice 5 — **Real outbound (one channel)**

| | |
| --- | --- |
| **Goal** | Replace mock with **one** real channel (e.g. SMTP, SendGrid, or provider API — pick one). |
| **Ships** | Adapter implementing idempotent send; secrets outside repo; throttle stub acceptable. |
| **Exit** | Real message delivered to a test inbox; `outreach.sent` with provider id in payload. |
| **Depends** | S4. |

---

### Slice 6 — **Reply path**

| | |
| --- | --- |
| **Goal** | Inbound reply → **Reply** row + `reply.classified`; categories from [canonical entities](../minilab/canonical-entities.md). |
| **Ships** | Webhook or polling ingest; classifier L0/L1 first; store `classifier_ingress_level`. |
| **Exit** | At least one classified reply tied to account/contact/correlation. |
| **Depends** | S5. |

---

### Slice 7 — **Closure + explainability pack**

| | |
| --- | --- |
| **Goal** | One **correlation** can be **replayed** from evidence (audit); minimal “why this touch” string or structured summary from policy + ledger. |
| **Ships** | Export or internal report: ordered kinds + ids for one `correlation_id`; optional hash chain fields if you committed to them. **Landed:** `minilab-store::audit` exports a sorted evidence trail, closure summary, touch explanation, and KPI snapshot; `GET /evidence/{correlation_id}/trail` exposes the replay surface. |
| **Exit** | Demo walkthrough: signal → send → reply with full trail; API response includes summary, explainability, and raw rows for operator review. |
| **Depends** | S6. |

---

### Slice 8 — **Daily queue + ingress ladder (automation)**

| | |
| --- | --- |
| **Goal** | Queue built **daily** without hand-holding; routine work avoids L3. |
| **Ships** | Scheduler (cron/worker); L0/L1 for routing/translation; L2 bounded; L3 logs to `premium_exception_event`-equivalent. |
| **Exit** | Matches week-2 DOD in [30-day rollout](gtm-30-day-rollout.md): autonomous queue; premium not steady-state. |
| **Depends** | S7. |

---

### Slice 9 — **Revenue loop (minimal)**

| | |
| --- | --- |
| **Goal** | Booking or handoff captured; stale detection or simple nurture state; weekly metrics slice. |
| **Ships** | Minimal booking table or CRM handoff; reports: top objections, family performance, stale rate. |
| **Exit** | Week-3-style outputs exist in a repeatable weekly artifact. |
| **Depends** | S8. |

---

### Slice 10 — **Hardening (production bar)**

| | |
| --- | --- |
| **Goal** | Legal/regional guardrails, opt-out propagation, throttles, premium justification quality, evidence spot-checks. |
| **Ships** | Policy scope `region` / `content` as needed; rate limits; constitutional review checklist from rollout week 4. |
| **Exit** | Day-30 success criteria in rollout doc; KPIs tracked. |
| **Depends** | S9. |

---

### Slice 11 — **v1 complete — declare wedge “real”**

| | |
| --- | --- |
| **Goal** | Formal acceptance of **End state (v1)** above; decide **next** horizon (second channel, new repo, app shell). |
| **Ships** | Written retro + metrics snapshot; optional tag/release of application + core versions. |
| **Exit** | Team agreement; backlog for post-v1. |
| **Depends** | S10. |

---

## Optional later (not required for v1)

| Slice | Idea |
| ----- | ---- |
| O1 | Second channel or second ICP (widens surface — new wedge rules). |
| O2 | `minilab` app repo split when [split criteria](../repo/layout-and-split-criteria.md) trigger. |
| O3 | Full Minilab distributed product (Places, hosts, mDNS, etc.) per architecture doc. |

---

## How long is this?

Depends on team and hours/week. **Rough** mapping: S1–S4 ≈ weeks 1–2 spine; S5–S7 ≈ weeks 2–3; S8–S10 ≈ weeks 3–4 and follow-on hardening — **if** you keep scope ruthlessly narrow. Slips usually come from **widening ICP/channel** or **skipping evidence**; the slice list is designed to make that visible early.

---

## Plan revision log

After each **major plan** change (or major reality change), append a row and edit the sections above as needed.

| Date | Summary of change | Link / notes |
| ---- | ----------------- | ------------ |
| *(initial)* | Baseline S0–S11 + optional O1–O3 as written. | — |
| 2026-05-11 | Landed Slice 7 replay/export surface: evidence trails, touch explanations, KPI snapshot helpers, and `/evidence/{correlation_id}/trail`. | `crates/minilab-store/src/audit.rs`, `crates/minilab-api/src/app.rs` |

**Related:** [Documentation index](../README.md) · [30-day rollout](gtm-30-day-rollout.md) · [Repository layout / split](../repo/layout-and-split-criteria.md)
