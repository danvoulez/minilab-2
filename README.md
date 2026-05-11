# Constitutional Runtime

Rust monorepo workspace: `**constitutional-runtime**` (IR, policy, lowering, evidence) plus `**minilab-core**` (business canon, elastic config, exploration, canonical entities). Documentation describes **Minilab** — intelligence proposes, policy and capability govern, the runtime lowers only what is **admissible**, and evidence legitimizes outcomes.


## Minilab CLI body

This branch adds `crates/minilab-cli`, producing the `minilab` binary. It is the local Rust-native command surface for LogLine compilation probes, walk/digest probes, receipt emission and validation, workorder prompt rendering, and the Max Automation probe map documented in [`docs/minilab/max-automation-map.md`](docs/minilab/max-automation-map.md).

The CLI intentionally starts with local, non-destructive receipts: Cloudflare remains projection/transport, SDKs remain machinery, LABs execute without governing, and receipts are the proof boundary.

Try:

```bash
minilab help
minilab logline compile examples/demo.logline
minilab receipt emit examples/demo.logline --out receipts
```

## Workspace


| Crate                                   | Role                                                                                      |
| --------------------------------------- | ----------------------------------------------------------------------------------------- |
| `constitutional-runtime`                | Canonical IR, validation, `OperationalCommand`, evidence stores                           |
| `minilab-core`                          | `BusinessCanon`; `ElasticOperatingConfig`; exploration; canonical entities; `Week1Step` |
| `minilab-store`                         | Supabase-backed ingest, scoring, policy, outbound, reply ingest, and evidence writes     |
| `minilab-api`                           | Permanent `axum` HTTP edge: health/ready, webhooks, `POST /outbound/send`, `POST /host-pairings/` |

Physical layout:

```text
/
  crates/
    constitutional-runtime/
    minilab-core/
    minilab-store/
    minilab-api/
  docs/
  migrations/
  scripts/
```


SQL for canonical tables: `[migrations/001_minilab_canonical.sql](migrations/001_minilab_canonical.sql)`. Try: `cargo run -p minilab-core --example week1_spine`.

## What `constitutional-runtime` provides


| Module       | Role                                                                                                                                                                  |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ir`         | Sixteen canonical IR primitives (`IRPrimitive`) and `IrNode` (shapes frozen at **v0.1**)                                                                              |
| `policy`     | Policy classes (A–D): see [`crates/constitutional-runtime/src/policy.rs`](crates/constitutional-runtime/src/policy.rs) — **D is reserved** (not executable at normal runtime boundary) |
| `capability` | `CapabilityManifest` (primitives, optional kind filter, `evidence.write` guarantee)                                                                                   |
| `evidence`   | `EvidenceContract`, `EvidenceRecord`, `EvidenceStore`, `FileEvidenceStore`; optional `SqliteEvidenceStore` / `SupabaseRestEvidenceStore` (features), `FailureToClose` |
| `lowering`   | `OperationalCommand` (**single canonical** command shape), `LoweringPlan`, `MinilabRuntimeLowerer`                                                                    |
| `decision`   | `DecideResolver`, `compile_flow` / `compile_node`, `resolve_lower_one`, `lower_compiled_flow` — `Decide` → concrete IR, then same lowerer                             |
| `validation` | `validate_structure`, `validate_policy`, `validate_capability`, `validate_admissibility`                                                                              |
| `ingress`    | `IngressMode` (fast path → premium exception)                                                                                                                         |


Execution is **not sovereign**: a command must be semantically valid, policy-permitted, capability-realizable, and evidentially accountable.

`**Decide`** is not lowered by `MinilabRuntimeLowerer`. Resolve it in `**decision**` (`compile_*` / `resolve_lower_one`) so the runtime lowerer never sees an unresolved `**Decide**`.

## Build

```bash
cargo test
cargo test --features sqlite-evidence
cargo test --features supabase-evidence
# optional: all evidence backends
cargo test --features "sqlite-evidence,supabase-evidence"
```

The repository root is a pure `[workspace]`; the runtime crate itself lives at [`crates/constitutional-runtime`](crates/constitutional-runtime).

For elevated server-side Supabase access, prefer `SUPABASE_SECRET_KEY` (`sb_secret_...`). `SUPABASE_SERVICE_KEY` remains supported as a legacy fallback for older projects.

## Documentation

Start at the **[documentation index](docs/README.md)** for reading order and a concept → doc map.


| Topic                              | Doc                                                                                                        |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Crate API & features               | [docs/runtime/crate-reference.md](docs/runtime/crate-reference.md)                                         |
| Constitutional runtime             | [docs/runtime/constitutional-runtime.md](docs/runtime/constitutional-runtime.md)                           |
| Minilab architecture               | [docs/minilab/architecture.md](docs/minilab/architecture.md)                                               |
| Minilab infra overview            | [docs/infra/overview.md](docs/infra/overview.md)                                                           |
| IR, lowering, validation, evidence | [docs/runtime/ir-and-lowering.md](docs/runtime/ir-and-lowering.md)                                         |
| Manifesto                          | [docs/minilab/manifesto.md](docs/minilab/manifesto.md)                                                     |
| Minilab narrative (full)           | [docs/minilab/business-runtime.md](docs/minilab/business-runtime.md)                                       |
| GTM 30-day rollout                 | [docs/operations/gtm-30-day-rollout.md](docs/operations/gtm-30-day-rollout.md)                             |
| Slice roadmap (v1 end)             | [docs/operations/slice-roadmap.md](docs/operations/slice-roadmap.md)                                       |
| Canonical GTM entities             | [docs/minilab/canonical-entities.md](docs/minilab/canonical-entities.md)                                   |
| Infra bring-up checklist          | [docs/infra/bringup-checklist.md](docs/infra/bringup-checklist.md)                                         |
| Repo layout & split criteria       | [docs/repo/layout-and-split-criteria.md](docs/repo/layout-and-split-criteria.md)                           |


## Completion criteria (this phase)

For every **runtime-lowerable** primitive: structural validation, policy validation, capability validation, lowering, and evidence kinds in `LoweringPlan`. Closure is explicit: `**close_execution_evidence`** returns `Err(FailureToClose)` if any evidence write fails.

## Status

**Constitutional Runtime Core (closed for this phase)** — Same as v0.1 operational coverage, plus: explicit `**PolicyClass::D`** semantics, `**decision**` compilation boundary, optional `**SupabaseRestEvidenceStore**` (`supabase-evidence`), golden tests for routing/checkpoint. **Milestone:** IR + admissibility + lowering + evidence closure + **decide resolution** + **real REST sink path** without a second command representation.
