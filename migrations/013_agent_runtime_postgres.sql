-- Agent runtime online persistence for the ChatGPT Place adapter.
--
-- These are first-class Postgres relations, not a generic jsonb envelope. The
-- adapter writes official sessions/runs/checkpoints and the corresponding
-- nine-slot LogLine rows online through Postgres/PostgREST.

CREATE TABLE IF NOT EXISTS agent_runtime_sessions (
    session_id uuid PRIMARY KEY,
    place_id text,
    app_id text,
    status text NOT NULL,
    provider_or_execution_substrate text,
    policy_state text,
    output_kind text,
    pending boolean NOT NULL DEFAULT false,
    opened_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS agent_runtime_runs (
    run_id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES agent_runtime_sessions(session_id) ON DELETE CASCADE,
    app_id text,
    status text,
    phase text,
    output_kind text,
    reply_text text,
    action_kind text,
    action_status text,
    governance_mode text,
    requires_confirmation boolean,
    outcome_label text,
    artifact_count integer NOT NULL DEFAULT 0,
    primary_artifact_label text,
    opened_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS agent_runtime_checkpoints (
    run_id uuid PRIMARY KEY REFERENCES agent_runtime_runs(run_id) ON DELETE CASCADE,
    session_id uuid NOT NULL REFERENCES agent_runtime_sessions(session_id) ON DELETE CASCADE,
    phase text,
    summary text,
    recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS agent_runtime_audit_events (
    session_id uuid NOT NULL REFERENCES agent_runtime_sessions(session_id) ON DELETE CASCADE,
    run_id uuid REFERENCES agent_runtime_runs(run_id) ON DELETE CASCADE,
    event_index integer NOT NULL,
    kind text,
    summary text NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, event_index)
);

CREATE TABLE IF NOT EXISTS runtime_loglines (
    logline_id text PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES agent_runtime_sessions(session_id) ON DELETE CASCADE,
    run_id uuid REFERENCES agent_runtime_runs(run_id) ON DELETE CASCADE,
    who text NOT NULL,
    did text NOT NULL,
    this_ref text NOT NULL,
    when_ref text NOT NULL DEFAULT 'online',
    confirmed_by text NOT NULL,
    if_ok text NOT NULL,
    if_doubt text NOT NULL,
    if_denied text NOT NULL,
    status text NOT NULL,
    output_kind text,
    policy_state text,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_sessions_place_updated
    ON agent_runtime_sessions(place_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_runs_session_opened
    ON agent_runtime_runs(session_id, opened_at DESC);

CREATE INDEX IF NOT EXISTS idx_runtime_loglines_session_recorded
    ON runtime_loglines(session_id, recorded_at DESC);
