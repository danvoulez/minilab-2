use serde_json::{json, Value};
use tracing::{info, info_span, Instrument};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};

/// Append an evidence row.
///
/// `idempotency_key` is optional. When set, the DB unique index on
/// `evidence_ledger.idempotency_key` guarantees at-most-once insert — duplicate
/// keys surface as a 409 from PostgREST, which we treat as a no-op success.
///
/// The payload is wrapped with the client's `sim_mode` so every ledger row
/// carries its execution context; production evidence is never silently mixed
/// with replay/simulation output.
pub async fn insert_ledger_row(
    client: &StoreClient,
    kind: &str,
    payload: Value,
    correlation_id: Uuid,
    idempotency_key: Option<String>,
) -> Result<(), StoreError> {
    let span = info_span!(
        "evidence.close",
        kind = %kind,
        correlation_id = %correlation_id,
        sim_mode = %client.sim_mode.as_str(),
        sim_branch = %client.sim_branch.branch_id,
    );
    let mut body = json!({
        "kind": kind,
        "payload": payload,
        "correlation_id": correlation_id,
        "sim_mode": client.sim_mode,
        "sim_branch_id": client.sim_branch.branch_id,
        "sim_parent_branch_id": client.sim_branch.parent_branch_id,
    });
    if let Some(key) = idempotency_key {
        body["idempotency_key"] = Value::String(key);
    }

    let resp = client
        .http
        .post(client.rest("evidence_ledger"))
        .json(&body)
        .send()
        .instrument(span.clone())
        .await?;

    let status = resp.status().as_u16();
    // 409 Conflict on unique idempotency_key → row already persisted, success.
    if status == 409 {
        info!(kind = %kind, correlation_id = %correlation_id, "evidence row already closed by idempotency key");
        return Ok(());
    }
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }
    info!(kind = %kind, correlation_id = %correlation_id, "evidence row closed");
    Ok(())
}
