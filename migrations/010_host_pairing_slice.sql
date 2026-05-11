-- Migration 010 — bundle-into-grammar §4 (host.pair slice).
--
-- Minimal `host` substrate so the constitutional `host.pair` orchestrator
-- has something to read. Deliberately narrow: no role, no assignment, no
-- capability_manifest, no lifecycle state machine, no agent_pubkey column.
-- Those concepts land in future bundle slices. See
-- `docs/integration/bundle-into-grammar.md` §5 (Deferred mappings).
--
-- The ledger side uses `host.pair.initiated`, `host.paired`, and
-- `host.pair_failed` kinds in the existing `evidence_ledger` — no new
-- table for pair events (invariant #2: no parallel evidence store).

create table if not exists host (
    id uuid primary key,
    registered_at timestamptz not null default now(),
    retired_at timestamptz,
    pairing_token_hash text,
    canon_version text not null default 'unknown',
    elastic_version text not null default 'unknown'
);

comment on table host is
    'Minimal host substrate for the host.pair constitutional slice. '
    'Real CapabilityManifest binding, role assignment, and lifecycle '
    'state machine are deferred per bundle-into-grammar §5.';
comment on column host.pairing_token_hash is
    'Operator-provided pre-authorization hash. Absence rejects host.pair '
    'with reason_code=token_mismatch (Policy class C escalation deferred).';
