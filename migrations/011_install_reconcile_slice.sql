-- Migration 011 — bundle-into-grammar §4 (install.reconcile slice).
--
-- The Reconcile-shaped slice keeps evidence in the existing `evidence_ledger`
-- (`install.reconcile.planned`, `install.reconcile.step.applied`,
-- `install.reconcile.reconciled`, `install.reconcile.failed`) and adds one
-- narrow state cache for the current applied manifest hash. The ledger remains
-- the source of constitutional truth; this table is operational substrate.

create table if not exists installation_state (
    installation_id uuid primary key,
    host_id uuid not null references host (id) on delete cascade,
    desired_hash text not null,
    applied_hash text,
    last_correlation_id uuid,
    updated_at timestamptz not null default now()
);

create index if not exists installation_state_host_idx
    on installation_state (host_id);

comment on table installation_state is
    'Operational cache for the install.reconcile slice. Constitutional history '
    'lives in evidence_ledger; this table only accelerates current-state lookup.';
