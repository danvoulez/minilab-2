ALTER TABLE reply
    ADD COLUMN IF NOT EXISTS correlation_id uuid,
    ADD COLUMN IF NOT EXISTS provider text NOT NULL DEFAULT 'unknown',
    ADD COLUMN IF NOT EXISTS raw_payload jsonb NOT NULL DEFAULT '{}'::jsonb;

UPDATE reply
SET correlation_id = COALESCE(correlation_id, uuid_generate_v4())
WHERE correlation_id IS NULL;

ALTER TABLE reply
    ALTER COLUMN correlation_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS reply_correlation_idx ON reply (correlation_id);
CREATE INDEX IF NOT EXISTS reply_provider_idx ON reply (provider);
