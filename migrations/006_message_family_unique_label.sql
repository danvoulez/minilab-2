-- Make message_family upserts idempotent on (campaign_id, label).
-- Pre-existing duplicate rows (if any) must be resolved before this migration.

CREATE UNIQUE INDEX IF NOT EXISTS message_family_campaign_label_unique
    ON message_family (campaign_id, label);
