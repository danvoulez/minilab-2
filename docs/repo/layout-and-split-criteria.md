# Repository layout and when to split

For now, **one repo is correct** — not forever, but at this stage it matches how the work is actually coupled.

## Why the same repo is right now

All of this still serves **one constitutional core**:

- the Rust **crates** (`constitutional-runtime`, `minilab-core`)
- **docs**
- **Minilab** pattern (manifesto, architecture)
- **GTM rollout**
- **canonical entities**
- **crate reference**

That is one body of thought, one implementation center, one reference surface.

## If you split too early

- Fake modularity
- Duplicated docs
- Drifting terminology
- Broken cross-links
- Version confusion
- More repo management than substance

Early splits are often net-negative.

## When to split (real rules)

Split only when **one** of these becomes true:

1. **Different release cadence**
  `constitutional-runtime` needs slow, careful versioning, but Minilab starts shipping changes daily.
2. **Different audience**
  One repo is for **public infrastructure** users; another is for **Minilab-specific operational** users.
3. **Different confidentiality**
  Minilab accumulates private GTM logic, prospecting configs, customer-specific policies, or operational secrets that should not sit beside the generic crate.
4. **Different artifact type**
  You move from **docs + core crate** to a **full app** — dashboards, jobs, storage adapters, a running product surface that is not just doctrine applied in documentation.

## Clean layout today (single repo)

Keep one repository with an **obvious** internal boundary:

```text
/
  crates/
    constitutional-runtime/
    minilab-core/
    minilab-store/
    minilab-api/
  migrations/           # Postgres / Supabase canonical schema
  docs/
    README.md
    runtime/
    minilab/
    operations/
    repo/
  README.md
```

## When you would split (target shape)

Split when you create **actual Minilab application code** that is no longer documentation around the constitutional runtime, but a **separate operational layer**.

**Repo A — `constitutional-runtime`**

- Generic core crate
- Generic docs
- Stable contracts

**Repo B — `minilab` (example name)**

- Company-runtime implementation
- GTM entities and migrations
- Department registry
- Apps / workflows
- Minilab-specific policies and operations

## Short answer

**Same repo now.** Separate repository later when Minilab is **code**, not only applied doctrine next to the core crate.

**Related:** [Documentation index](../README.md) · [Minilab architecture](../minilab/architecture.md)
