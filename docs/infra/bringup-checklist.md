# Minilab Infra Bring-up Checklist

This checklist is intentionally narrow. It exists to get the isolated two-host infra working reliably, with `LAB 256` as the admin machine.

## Phase 1 — Identity

Each machine must have:

- stable `host_id`
- canonical host name
- role
- Place binding
- credential state in Supabase

Exit:

- `lab8gb` and `lab512` exist in Supabase as canonical network hosts
- `lab256` exists as the canonical admin machine / control surface
- each has an explicit role and Place binding

## Phase 2 — External source of truth

Supabase must be the external operational truth for:

- host records
- heartbeats
- commands
- evidence
- install state
- artifact metadata

Exit:

- a host can reboot or reinstall and recover its state from Supabase-backed truth

## Phase 3 — Install wizard

The canonical install flow must be wizard-driven:

1. choose target machine
2. identify host role
3. perform pairing / approval
4. issue credentials
5. register or reconcile host in Supabase
6. select signed artifact
7. install
8. verify
9. emit evidence

Exit:

- one new or reset network host can be brought up end-to-end without ad hoc shell choreography

## Phase 4 — Signed artifacts

Artifacts must be:

- published in Supabase Storage
- signed
- verifiable before install
- tied to release/install state in Supabase

Exit:

- one host installs from a signed artifact referenced by Supabase, not from a local copy

## Phase 5 — One real loop

The first loop that matters:

1. UI launches `host.verify target=<host>`
2. runtime admits
3. target machine executes
4. evidence writes to Supabase
5. UI shows final state

Exit:

- one command path is visibly real end-to-end

## Refusal rule

Do not expand scope into broader product ideas until the following are true:

- wizard install works
- signed update works
- one command/evidence loop works
- UI shows health and last action
