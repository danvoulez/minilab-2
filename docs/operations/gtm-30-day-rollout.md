# Minilab GTM — 30-day rollout

Operational wedge: **one ICP, one channel, one offer**, with constitutional evidence and policy discipline. This is the operational companion to [constitutional runtime](../runtime/constitutional-runtime.md) and [canonical entities](../minilab/canonical-entities.md).

## Non-negotiables (month one)

- Do not add multiple ICPs or multiple outbound channels.
- Do not let premium become the easy path (ingress ladder in [manifesto](../minilab/manifesto.md)).
- Do not let the runtime invent novel claims; messaging draws only from a **policy-gated claims library** (see `ClaimsProfile` in canonical entities).
- Do not collapse all departments into one undifferentiated agent; keep **jurisdictions** visible in routing and evidence.
- Do not make evidence closure optional for material actions (use `FailureToClose` semantics from the crate, not silent success).

## Week 1 — mini company spine

**Goal:** A single-threaded GTM pipeline that exists in code and data, not only in slides.

### Define (once; freeze 30 days)

- One **ICP** segment label (stored on `Account.icp_segment`).
- One **buyer persona** (drives message families and qualification).
- One **core pain** hypothesis (used in copy and routing).
- One **offer** (what the booking path promises).
- One **outbound channel** only (e.g. email or LinkedIn — pick one).
- One **booking path** (calendar link, form, or handoff rule).

### Departments (staff first)

Research · Prospect graph / Sales Ops · Marketing · SDR / Outreach · Compliance / Constitutional · Memory / Analytics.

Each department owns a narrow slice of the graph: Research ingests signals; Ops maintains account truth; Marketing owns message families and experiments; SDR owns sequences and sends; Compliance owns rules and premium governance; Memory owns reporting and stale detection.

### Workflows (ship in this order)

1. Market **signal** ingestion (collector → `TriggerEvent`).
2. **Account** create / merge / dedupe.
3. **Account scoring** (ranking for queue).
4. **Message-family** generation (approved claims only).
5. **Outreach send** (bounded by policy and opt-out).
6. **Reply** classification.
7. **Evidence closure** at correlation boundaries (every material step has a `kind`).

### Definition of done

- At least **one account** runs end-to-end through the above steps in production-like data.
- Every material action writes an **evidence** row (or fails closed).
- **Opt-out** and stop rules **hard-block** sends (`policy.blocked`, never spoof `outreach.sent` for blocked contacts).

---

## Week 2 — automate

**Goal:** The daily queue runs without manual copy-paste; routine work stays off premium.

### Build

- Signal **collectors** (RSS, webhooks, CSV drops, or API — pick what matches channel).
- **Ranking** and queue ordering from score + policy.
- **Buyer-role inference** and **trigger classification** (L0/L1 first).
- First **message** and **landing** generators tied to `MessageFamily` / `template_ref`.
- **Follow-up scheduler** (`Sequence` states: pending → sent or blocked).

### GTM ingress

- **L0** deterministic rules.
- **L1** constrained translator (cheap model or templates).
- **L2** Place Agent (bounded tool use).
- **L3** premium — **logged**, **budget-gated**, **justified** (`PremiumExceptionEvent`).

### Definition of done

- Daily queue is produced **autonomously** from signals + rules.
- **Routine replies** and follow-ups do not require human composition for the happy path.
- Premium is **not** in steady-state volume (watch the premium escalation KPI).

---

## Week 3 — revenue loop

**Goal:** Connect outreach to meetings and learning; close the loop from send → reply → book → nurture.

### Add

- **Booking** capture and handoff.
- **Nurture** tracks for non-responders and long-cycle accounts.
- **Objection memory** (categories feed back into message families).
- **Campaign comparison** (which families win per segment).
- **Stale account** detection and re-engagement or archival rules.
- **Message-family metrics** and **segment** reports for founder review.

### Weekly outputs (standing agenda)

- Top opportunities and **objections**.
- **Winning** message families vs losers.
- **Underperforming** segments.
- **Premium** exceptions (count, reasons, cost).
- **Compliance** incidents (policy blocks, edge cases).

### Definition of done

- For a sampled set of touches, you can **explain** why the account was contacted and **why** that message variant was chosen (policy + evidence pointers).
- Bad campaigns are **stopped** or throttled without heroics.
- Funnel **leaks** (drops between signal → send → reply → book) are visible in reports.

---

## Week 4 — harden

**Goal:** Production safety, audits, and constitutional hygiene before scaling surface area.

### Tighten

- Legal / **regional** constraints on sends (PolicyRule scope `region`).
- **Opt-out propagation** across systems of record (single source of truth for `Contact.opt_out`).
- **Throttles** (rate limits, cap per domain, cooling-off).
- **Strategic** account escalation (human review path).
- **Premium justification** quality (every L3 traceable).
- **Evidence audits** (hash chain or spot-check completeness).
- **Claims library** versioning (no silent drift of approved claims).

### KPIs (review weekly)


| KPI                     | Intent                                             |
| ----------------------- | -------------------------------------------------- |
| Autonomy rate           | Share of steps without human-in-the-loop           |
| Evidence closure rate   | Material actions with successful evidence write    |
| Positive reply rate     | Quality of air cover                               |
| Premium escalation rate | Should trend toward **~0** in healthy steady state |
| Stop-rule compliance    | Blocks enforced vs bypass attempts                 |
| Stale rate              | Accounts going cold without policy                 |
| Campaign decay          | When to kill or refresh a campaign                 |


### Weekly constitutional review (standing questions)

- Is the **runtime** overreaching (executing what should be deliberation)?
- Are **premium** calls justified and shrinking?
- Can we **compile** repeated premium patterns into cheaper structure (new rules, templates, L0/L1)?
- Any **boundary leaks** between departments or evidence gaps?

### Definition of done

- The wedge runs **daily** without babysitting.
- Premium is **rare** and **justified** when used.
- **Policy blocks** are real (not bypassed by convenience).
- **Evidence** is complete enough for replay and audit.
- **One wedge** — still one ICP, one channel, one offer — proven end-to-end.

---

## Day-30 success

**Detect → rank → approved outreach → send → classify replies → follow up → book / nurture → explain every action → near-zero premium usage.**

Success is structural: admissible execution, closed evidence, and intelligence **compiling downward** into cheaper runtime — not a higher model spend.

**Related:** [Documentation index](../README.md) · [Manifesto](../minilab/manifesto.md) · [Architecture](../minilab/architecture.md)
