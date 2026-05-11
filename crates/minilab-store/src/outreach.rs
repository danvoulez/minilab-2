use serde_json::json;
use uuid::Uuid;

use minilab_core::EvidenceKind;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;
use crate::outbound::DeliveryReceipt;

fn sequence_evidence_key(kind: &str, sequence_id: Uuid) -> String {
    format!("{kind}:{sequence_id}")
}

fn validate_existing_state(
    sequence_id: Uuid,
    current_state: Option<&str>,
    desired_state: &str,
) -> Result<(), StoreError> {
    match current_state {
        Some(state) if state == desired_state => Ok(()),
        Some(state) => Err(StoreError::Supabase {
            status: 409,
            body: format!(
                "sequence {sequence_id} transition refused: expected pending -> {desired_state}, found {state}"
            ),
        }),
        None => Err(StoreError::Supabase {
            status: 404,
            body: format!("sequence {sequence_id} not found"),
        }),
    }
}

async fn transition_sequence_state(
    client: &StoreClient,
    sequence_id: Uuid,
    desired_state: &str,
) -> Result<(), StoreError> {
    let resp = client
        .http
        .patch(format!(
            "{}?id=eq.{}&state=eq.pending",
            client.rest("sequence"),
            sequence_id
        ))
        .header("Prefer", "return=representation")
        .json(&json!({ "state": desired_state, "updated_at": chrono::Utc::now() }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let rows: Vec<serde_json::Value> = resp.json().await?;
    if !rows.is_empty() {
        return Ok(());
    }

    let current_rows: Vec<serde_json::Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=state&limit=1",
            client.rest("sequence"),
            sequence_id
        ))
        .send()
        .await?
        .json::<Vec<serde_json::Value>>()
        .await?;

    let current_state = current_rows.first().and_then(|row| row["state"].as_str());
    validate_existing_state(sequence_id, current_state, desired_state)
}

/// Transition a sequence row to `sent` and write `outreach.sent` evidence.
pub async fn record_successful_send(
    client: &StoreClient,
    sequence_id: Uuid,
    account_id: Uuid,
    contact_id: Uuid,
    campaign_id: Uuid,
    correlation_id: Uuid,
    receipt: &DeliveryReceipt,
) -> Result<(), StoreError> {
    transition_sequence_state(client, sequence_id, "sent").await?;

    insert_ledger_row(
        client,
        EvidenceKind::OUTREACH_SENT,
        json!({
            "sequence_id": sequence_id,
            "account_id": account_id,
            "contact_id": contact_id,
            "campaign_id": campaign_id,
            "channel": receipt.channel,
            "provider": receipt.provider,
            "provider_message_id": receipt.provider_message_id,
            "template_ref": receipt.template_ref,
            "message_family_id": receipt.message_family_id,
            "mock": receipt.mock,
            "operational_command": {
                "namespace": receipt.operational_command.namespace,
                "verb": receipt.operational_command.verb,
                "args": receipt.operational_command.args,
                "target_runtime": receipt.operational_command.target_runtime,
            },
        }),
        correlation_id,
        Some(sequence_evidence_key(
            EvidenceKind::OUTREACH_SENT,
            sequence_id,
        )),
    )
    .await
}

/// Transition a sequence row to `blocked` and write `policy.blocked` evidence.
pub async fn block_send(
    client: &StoreClient,
    sequence_id: Uuid,
    contact_id: Uuid,
    reason: &str,
    correlation_id: Uuid,
) -> Result<(), StoreError> {
    transition_sequence_state(client, sequence_id, "blocked").await?;

    insert_ledger_row(
        client,
        EvidenceKind::POLICY_BLOCKED,
        json!({
            "sequence_id": sequence_id,
            "contact_id": contact_id,
            "reason": reason,
        }),
        correlation_id,
        Some(sequence_evidence_key(
            EvidenceKind::POLICY_BLOCKED,
            sequence_id,
        )),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{sequence_evidence_key, validate_existing_state};
    use minilab_core::EvidenceKind;
    use uuid::Uuid;

    #[test]
    fn outreach_key_is_deterministic_per_sequence() {
        assert_eq!(
            sequence_evidence_key(EvidenceKind::OUTREACH_SENT, Uuid::nil()),
            "outreach.sent:00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn same_terminal_state_is_idempotent() {
        assert!(validate_existing_state(Uuid::nil(), Some("sent"), "sent").is_ok());
    }

    #[test]
    fn conflicting_terminal_state_is_rejected() {
        let err = validate_existing_state(Uuid::nil(), Some("blocked"), "sent").unwrap_err();
        assert!(err.to_string().contains("transition refused"));
    }
}
