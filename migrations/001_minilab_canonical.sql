-- Minilab month-one canonical entities (Postgres / Supabase).
-- See docs/minilab-canonical-entities.md

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS citext;

-- --- Core GTM ---

CREATE TABLE account (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    name text NOT NULL,
    domain text NOT NULL,
    icp_segment text NOT NULL,
    score double precision NOT NULL DEFAULT 0,
    stage text NOT NULL DEFAULT 'new',
    strategic_flag boolean NOT NULL DEFAULT false,
    dedupe_key text NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE contact (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    account_id uuid NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    email citext NOT NULL,
    role_inferred text,
    buyer_persona_id text,
    opt_out boolean NOT NULL DEFAULT false,
    opt_out_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX contact_account_idx ON contact (account_id);
CREATE UNIQUE INDEX contact_email_unique ON contact (email);

CREATE TABLE trigger_event (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    account_id uuid REFERENCES account (id) ON DELETE SET NULL,
    source text NOT NULL,
    signal_type text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX trigger_event_account_idx ON trigger_event (account_id);

CREATE TABLE claims_profile (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    name text NOT NULL,
    claim_ids text[] NOT NULL DEFAULT ARRAY[]::text[],
    version int NOT NULL DEFAULT 1,
    active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE campaign (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    name text NOT NULL,
    channel text NOT NULL,
    status text NOT NULL,
    claims_profile_id uuid NOT NULL REFERENCES claims_profile (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE message_family (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE,
    label text NOT NULL,
    template_ref text NOT NULL,
    claims_allowed text[] NOT NULL DEFAULT ARRAY[]::text[],
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX message_family_campaign_idx ON message_family (campaign_id);

CREATE TABLE sequence (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    campaign_id uuid NOT NULL REFERENCES campaign (id) ON DELETE CASCADE,
    account_id uuid NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    contact_id uuid NOT NULL REFERENCES contact (id) ON DELETE CASCADE,
    step_index int NOT NULL,
    scheduled_at timestamptz NOT NULL,
    state text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX sequence_scheduled_idx ON sequence (scheduled_at) WHERE state = 'pending';

CREATE TABLE reply (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    account_id uuid NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    contact_id uuid NOT NULL REFERENCES contact (id) ON DELETE CASCADE,
    channel_message_id text NOT NULL,
    body text NOT NULL,
    category text NOT NULL,
    classifier_ingress_level int NOT NULL DEFAULT 0,
    received_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (channel_message_id)
);

-- Append-only business evidence ledger (application-level; aligns with runtime evidence contracts)
CREATE TABLE evidence_ledger (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    kind text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    correlation_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX evidence_ledger_correlation_idx ON evidence_ledger (correlation_id);
CREATE INDEX evidence_ledger_kind_idx ON evidence_ledger (kind);

CREATE TABLE policy_rule (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    name text NOT NULL,
    scope text NOT NULL,
    expression jsonb NOT NULL DEFAULT '{}'::jsonb,
    priority int NOT NULL DEFAULT 0,
    active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE premium_exception_event (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    reason_code text NOT NULL,
    justification text NOT NULL,
    budget_unit text NOT NULL,
    cost_estimate double precision,
    ingress_context jsonb NOT NULL DEFAULT '{}'::jsonb,
    correlation_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX premium_exception_correlation_idx ON premium_exception_event (correlation_id);
