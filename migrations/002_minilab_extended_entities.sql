-- Extended entities: buying_signal, meeting, department_run

CREATE TABLE buying_signal (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    account_id uuid NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    kind text NOT NULL,
    strength double precision NOT NULL DEFAULT 0,
    source text NOT NULL,
    evidence_ref uuid,
    observed_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX buying_signal_account_idx ON buying_signal (account_id);

CREATE TABLE meeting (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    account_id uuid NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    contact_id uuid NOT NULL REFERENCES contact (id) ON DELETE CASCADE,
    campaign_id uuid REFERENCES campaign (id) ON DELETE SET NULL,
    status text NOT NULL DEFAULT 'requested',
    scheduled_for timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX meeting_account_idx ON meeting (account_id);

CREATE TABLE department_run (
    id uuid PRIMARY KEY DEFAULT uuid_generate_v4(),
    department_kind text NOT NULL,
    input_ref uuid,
    output_ref uuid,
    status text NOT NULL DEFAULT 'running',
    evidence_ref uuid,
    correlation_id uuid NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz
);

CREATE INDEX department_run_correlation_idx ON department_run (correlation_id);
CREATE INDEX department_run_status_idx ON department_run (status);
