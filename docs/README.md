# Documentation index

The repository docs are grouped by **jurisdiction**:

- `runtime/` — constitutional semantics and crate surface
- `infra/` — physical Minilab network, bring-up, Places, install/update model
- `integration/` — domain and substrate integration memos at the grammar boundary
- `minilab/` — company canon, architecture, and entity contracts
- `operations/` — rollout, slices, and operator planning
- `repo/` — monorepo structure and split criteria

Read in this order if you are new:


| Order | Document                                                       | What it is                                                                      |
| ----- | -------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| 1     | [minilab/manifesto.md](minilab/manifesto.md)                   | Why Minilab exists — one-page summary                                           |
| 2     | [minilab/business-runtime.md](minilab/business-runtime.md)     | Full narrative: problem, layers, GTM wedge, proof standard, build sequence      |
| 3     | [runtime/constitutional-runtime.md](runtime/constitutional-runtime.md) | Core execution philosophy (IR, policy, evidence, separation of powers) |
| 4     | [runtime/ir-and-lowering.md](runtime/ir-and-lowering.md)       | Primitives, `OperationalCommand`, validation passes, decision compile, evidence |
| 4b    | [infra/overview.md](infra/overview.md)                         | Physical Minilab infra: two isolated hosts, one admin machine, Places, Supabase truth |
| 4c    | [infra/bringup-checklist.md](infra/bringup-checklist.md)       | Brutally practical bring-up sequence for the isolated host infra |
| 5     | [minilab/architecture.md](minilab/architecture.md)             | Distributed Minilab stack vs this crate’s boundary                              |
| 6     | [operations/gtm-30-day-rollout.md](operations/gtm-30-day-rollout.md) | Four-week GTM wedge plan, KPIs, DOD                                    |
| 6b    | [operations/slice-roadmap.md](operations/slice-roadmap.md)     | Vertical slices S0–S11 to “end” v1 wedge + optional later horizons              |
| 6c    | [integration/chatgpt-place-adapter.md](integration/chatgpt-place-adapter.md) | Operational memo for the ChatGPT-backed premium-assisted Place         |
| 7     | [minilab/canonical-entities.md](minilab/canonical-entities.md) | GTM data model, evidence kinds, opt-out rules                                   |
| 8     | [runtime/crate-reference.md](runtime/crate-reference.md)       | Rust crate modules, features, key types                                         |
| —     | [repo/layout-and-split-criteria.md](repo/layout-and-split-criteria.md) | Why one repo now; when and how to split later                           |
| —     | [../migrations/README.md](../migrations/README.md)             | Migration order, rationale, and operational notes                               |
| —     | [../migrations/001_minilab_canonical.sql](../migrations/001_minilab_canonical.sql) | Initial Postgres DDL for canonical GTM entities                      |


## Map: concept → doc


| Concept                                     | Primary doc                                                                                          |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| PolicyClass A/B/C/D (D reserved at runtime) | [runtime/ir-and-lowering.md](runtime/ir-and-lowering.md), [runtime/constitutional-runtime.md](runtime/constitutional-runtime.md) |
| `Decide` vs `MinilabRuntimeLowerer`         | [runtime/ir-and-lowering.md](runtime/ir-and-lowering.md) |
| `DecideResolver`, `compile_flow`            | [runtime/ir-and-lowering.md](runtime/ir-and-lowering.md), [runtime/crate-reference.md](runtime/crate-reference.md) |
| Evidence closure, `FailureToClose`          | [runtime/ir-and-lowering.md](runtime/ir-and-lowering.md), [runtime/constitutional-runtime.md](runtime/constitutional-runtime.md) |
| GTM tables & workflows                      | [minilab/canonical-entities.md](minilab/canonical-entities.md) |
| Ingress ladder (L0–L3)                      | [minilab/manifesto.md](minilab/manifesto.md), [minilab/business-runtime.md](minilab/business-runtime.md), [runtime/constitutional-runtime.md](runtime/constitutional-runtime.md) |
| ChatGPT-backed premium-assisted Place       | [integration/chatgpt-place-adapter.md](integration/chatgpt-place-adapter.md) |
| Infra bring-up and install/update discipline | [infra/overview.md](infra/overview.md), [infra/bringup-checklist.md](infra/bringup-checklist.md) |
| Full Minilab thesis (narrative)             | [minilab/business-runtime.md](minilab/business-runtime.md) |
| Slice plan (v1 end + optional O1–O3)        | [operations/slice-roadmap.md](operations/slice-roadmap.md) |
| Monorepo vs future split                    | [repo/layout-and-split-criteria.md](repo/layout-and-split-criteria.md) |


## Glossary (short)


| Term                 | Meaning                                                                            |
| -------------------- | ---------------------------------------------------------------------------------- |
| ICP                  | Ideal customer profile — one segment for month one (`Account.icp_segment`).        |
| Admissible           | Passes structure, policy, capability, and evidence contract checks before execute. |
| `OperationalCommand` | Single lowered shape: namespace + verb + args + target runtime.                    |
| Evidence closure     | Material success requires successful evidence write, or `FailureToClose`.          |
| Ingress (L0–L3)      | L0 deterministic → L1 cheap translator → L2 Place Agent → L3 premium exception.    |
| Place Agent          | Bounded L2 automation (see manifesto / architecture docs).                         |
| Claims profile       | Approved claims set for a campaign; no novel claims at send time.                  |
| Correlation id       | Ties steps in one workflow run (see `EvidenceRecord` + application rows).          |


## Root project README

The repository [README.md](../README.md) summarizes the Rust crate, build commands, and links here.
