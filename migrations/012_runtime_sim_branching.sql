-- Migration 012 — runtime-level SimMode branching.
--
-- Evidence rows now carry a branch identity in addition to sim_mode so
-- replay/simulation/counterfactual executions can fork state without touching
-- the production evidence line. Production rows use branch_id='production';
-- counterfactual forks set sim_parent_branch_id to the branch they diverged
-- from.

alter table evidence_ledger
    add column if not exists sim_branch_id text not null default 'production',
    add column if not exists sim_parent_branch_id text;

create index if not exists evidence_ledger_sim_branch_idx
    on evidence_ledger (sim_mode, sim_branch_id);

create index if not exists evidence_ledger_sim_parent_branch_idx
    on evidence_ledger (sim_parent_branch_id)
    where sim_parent_branch_id is not null;
