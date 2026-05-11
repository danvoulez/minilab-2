-- Idempotency key + sim_mode tagging for evidence_ledger.
--
-- idempotency_key: callers pass a deterministic key when they want
-- at-most-once semantics; NULL means "no dedupe" and multiple rows are
-- allowed (legacy behavior). Duplicate inserts surface as 409 Conflict.
--
-- sim_mode: production evidence is never silently mixed with replay or
-- simulation output. Every ledger row is tagged with the StoreClient's
-- execution mode at write time.

ALTER TABLE evidence_ledger
    ADD COLUMN IF NOT EXISTS idempotency_key text;

ALTER TABLE evidence_ledger
    ADD COLUMN IF NOT EXISTS sim_mode text NOT NULL DEFAULT 'production';

CREATE UNIQUE INDEX IF NOT EXISTS evidence_ledger_idempotency_unique
    ON evidence_ledger (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS evidence_ledger_sim_mode_idx
    ON evidence_ledger (sim_mode);
