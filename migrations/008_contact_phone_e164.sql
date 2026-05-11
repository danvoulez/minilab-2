ALTER TABLE contact
    ADD COLUMN IF NOT EXISTS phone_e164 text;

CREATE UNIQUE INDEX IF NOT EXISTS contact_phone_e164_unique
    ON contact (phone_e164)
    WHERE phone_e164 IS NOT NULL;
