# Minilab Infra Overview

Minilab infra is the **personal constitutional network** that the user administers directly.

It is not the commercial company runtime. It is the **body** that the UI governs and that the constitutional runtime adjudicates.

## What it is

- two isolated network hosts: `LAB 8GB` and `LAB 512`
- one admin machine: `LAB 256`
- one **Places** UI
- observability
- chat with the physical infra
- workflows
- control apps
- local-LLM execution surfaces

`LAB 256` is the user's computer and primary admin/control surface. It is not part of the isolated Minilab network body in the same way that `LAB 8GB` and `LAB 512` are.

## What governs it

- [`constitutional-runtime`](../../crates/constitutional-runtime) as the engine/law
- a signed artifact and manifest discipline
- external Supabase as the durable operational source of truth

## Non-negotiables

### 1. External Supabase is the infra source of truth

Infra state must not live primarily in local ad hoc files or implicit machine memory.

Supabase is the external source of truth for:

- host identity
- place bindings
- commands
- leases
- health / heartbeat
- install state
- rollout state
- evidence

This is a hard requirement because the infra must remain recoverable even when local install state is broken or incomplete.

### 2. Infra installation must happen through a wizard

Manual bring-up has already failed repeatedly. The install path therefore must be treated as a product surface, not a shell ritual.

The canonical install path is:

- discover or bind the target machine
- identify the host role
- pair / approve / issue credentials
- bind Place
- register host in Supabase
- fetch signed artifact metadata
- install
- verify
- publish evidence

If a human cannot reliably bring up one machine through the wizard, the infra is not production-ready.

### 3. Updates ship as signed artifacts from Supabase Storage

Infra updates are not loose local copies.

They must come from:

- signed artifacts
- published to Supabase Storage
- validated against the runtime/manifest trust model before install

This is how the system avoids repeating fragile manual deployment attempts.

### 4. UI is a requirement, not a later enhancement

The user now needs UI, not another hidden operational layer.

The UI must be able to reveal at minimum:

- host health
- readiness
- last command
- last evidence
- install / rollout state
- pairing / enrollment status

The UI is the operational face of the infra, but it is not the authority. Authority still runs through the constitutional runtime.

## The clean split

### Minilab infra

- personal admin network
- isolated physical hosts
- Places
- workflows
- observability
- control surface

Canonical topology:

- `LAB 8GB` — network host
- `LAB 512` — network host
- `LAB 256` — admin computer / control surface

### Huge Mini Company

- commercial operator
- runs on top of the infra
- also uses the constitutional runtime
- has its own canon and elastic operating layer

## Immediate implication

The bottleneck is infra materialization, not more runtime philosophy.

The next necessary loop is:

1. UI issues or launches a bounded action
2. runtime admits it
3. one target host executes it
4. evidence returns to Supabase
5. UI reflects the resulting state
