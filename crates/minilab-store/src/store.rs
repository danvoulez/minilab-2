use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use minilab_core::EvidenceKind;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

/// Minimal signal descriptor fed to the ingest pipeline.
#[derive(Debug, Serialize, Deserialize)]
pub struct IngestInput {
    /// e.g. "linkedin_job_post", "website_visit", "manual"
    pub signal_type: String,
    /// Signal origin (e.g. "linkedin", "clearbit", "cli")
    pub source: String,
    /// Company domain — used as the account dedupe key.
    pub domain: String,
    /// Company name (used for account upsert if new).
    pub account_name: String,
    /// ICP segment identifier.
    pub icp_segment: String,
    /// Arbitrary extra signal payload.
    #[serde(default)]
    pub payload: Value,
}

/// Row IDs produced by a successful ingest run.
#[derive(Debug, Serialize, Deserialize)]
pub struct IngestOutput {
    pub correlation_id: Uuid,
    pub account_id: Uuid,
    pub trigger_event_id: Uuid,
}

fn account_evidence_kind(inserted: bool) -> &'static str {
    if inserted {
        EvidenceKind::ACCOUNT_CREATED
    } else {
        EvidenceKind::ACCOUNT_MERGED
    }
}

/// Full ingest pipeline: signal → trigger_event + account upsert + evidence.
///
/// Exit criterion for Slice 2: writes `trigger.ingested` + `account.created`
/// (or `account.merged`) to `evidence_ledger` for the given correlation.
pub async fn ingest_signal(
    client: &StoreClient,
    input: IngestInput,
) -> Result<IngestOutput, StoreError> {
    let correlation_id = Uuid::new_v4();
    let now = Utc::now();

    // 1. Upsert account via RPC so we know whether this run inserted or merged.
    let account_body = json!({
        "p_name": input.account_name,
        "p_domain": input.domain,
        "p_icp_segment": input.icp_segment,
        "p_dedupe_key": input.domain,
    });

    let acct_resp = client
        .http
        .post(client.rpc("minilab_upsert_account"))
        .header("Prefer", "return=representation")
        .json(&account_body)
        .send()
        .await?;

    let acct_status = acct_resp.status().as_u16();
    if acct_status >= 300 {
        let text = acct_resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase {
            status: acct_status,
            body: text,
        });
    }

    let acct_rows: Vec<Value> = acct_resp.json().await?;
    let account = acct_rows.into_iter().next().unwrap_or_default();
    let account_id: Uuid =
        serde_json::from_value(account["id"].clone()).unwrap_or_else(|_| Uuid::new_v4());
    let inserted = account["inserted"].as_bool().unwrap_or(false);

    // 2. Insert trigger_event.
    let trigger_body = json!({
        "account_id": account_id,
        "source": input.source,
        "signal_type": input.signal_type,
        "payload": input.payload,
        "occurred_at": now,
    });

    let trig_resp = client
        .http
        .post(client.rest("trigger_event"))
        .header("Prefer", "return=representation")
        .json(&trigger_body)
        .send()
        .await?;

    let trig_status = trig_resp.status().as_u16();
    if trig_status >= 300 {
        let text = trig_resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase {
            status: trig_status,
            body: text,
        });
    }

    let trig_rows: Vec<Value> = trig_resp.json().await?;
    let trigger_event_id: Uuid = trig_rows
        .into_iter()
        .next()
        .and_then(|r| serde_json::from_value(r["id"].clone()).ok())
        .unwrap_or_else(Uuid::new_v4);

    // 3. Evidence: trigger.ingested
    insert_ledger_row(
        client,
        EvidenceKind::TRIGGER_INGESTED,
        json!({
            "trigger_event_id": trigger_event_id,
            "signal_type": input.signal_type,
            "source": input.source,
            "domain": input.domain,
        }),
        correlation_id,
        Some(format!("trigger.ingested:{trigger_event_id}")),
    )
    .await?;

    // 4. Evidence: account.created or account.merged
    let acct_kind = account_evidence_kind(inserted);
    insert_ledger_row(
        client,
        acct_kind,
        json!({
            "account_id": account_id,
            "domain": input.domain,
            "icp_segment": input.icp_segment,
        }),
        correlation_id,
        Some(format!("{acct_kind}:{account_id}:{trigger_event_id}")),
    )
    .await?;

    Ok(IngestOutput {
        correlation_id,
        account_id,
        trigger_event_id,
    })
}

#[cfg(test)]
mod tests {
    use super::account_evidence_kind;
    use minilab_core::EvidenceKind;

    #[test]
    fn inserted_accounts_emit_created_evidence() {
        assert_eq!(account_evidence_kind(true), EvidenceKind::ACCOUNT_CREATED);
    }

    #[test]
    fn merged_accounts_emit_merged_evidence() {
        assert_eq!(account_evidence_kind(false), EvidenceKind::ACCOUNT_MERGED);
    }
}
