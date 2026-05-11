-- Deterministic account upsert: returns the resulting row and whether it was
-- newly inserted. Replaces the `created_at == updated_at` heuristic in
-- the application layer, which was racy and broke on same-second re-runs.

CREATE OR REPLACE FUNCTION minilab_upsert_account(
    p_name text,
    p_domain text,
    p_icp_segment text,
    p_dedupe_key text
) RETURNS TABLE (
    id uuid,
    name text,
    domain text,
    icp_segment text,
    score double precision,
    stage text,
    strategic_flag boolean,
    dedupe_key text,
    metadata jsonb,
    created_at timestamptz,
    updated_at timestamptz,
    inserted boolean
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_id uuid;
    v_existing_id uuid;
BEGIN
    SELECT a.id INTO v_existing_id
    FROM account a
    WHERE a.dedupe_key = p_dedupe_key
    LIMIT 1;

    IF v_existing_id IS NULL THEN
        INSERT INTO account (name, domain, icp_segment, dedupe_key)
        VALUES (p_name, p_domain, p_icp_segment, p_dedupe_key)
        RETURNING account.id INTO v_id;

        RETURN QUERY
        SELECT a.id, a.name, a.domain, a.icp_segment, a.score, a.stage,
               a.strategic_flag, a.dedupe_key, a.metadata,
               a.created_at, a.updated_at, true
        FROM account a WHERE a.id = v_id;
    ELSE
        UPDATE account a
        SET name        = COALESCE(NULLIF(p_name, ''), a.name),
            icp_segment = COALESCE(NULLIF(p_icp_segment, ''), a.icp_segment),
            updated_at  = now()
        WHERE a.id = v_existing_id;

        RETURN QUERY
        SELECT a.id, a.name, a.domain, a.icp_segment, a.score, a.stage,
               a.strategic_flag, a.dedupe_key, a.metadata,
               a.created_at, a.updated_at, false
        FROM account a WHERE a.id = v_existing_id;
    END IF;
END;
$$;
