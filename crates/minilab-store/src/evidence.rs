use serde_json::{json, Value};
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
    let mut body = json!({
        "kind": kind,
        "payload": payload,
        "correlation_id": correlation_id,
        "sim_mode": client.sim_mode,
    });
    if let Some(key) = idempotency_key {
        body["idempotency_key"] = Value::String(key);
    }

    let resp = client
        .http
        .post(client.rest("evidence_ledger"))
        .json(&body)
        .send()
        .await?;

    let status = resp.status().as_u16();
    // 409 Conflict on unique idempotency_key → row already persisted, success.
    if status == 409 {
        return Ok(());
    }
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }
    Ok(())
}
