use serde_json::{json, Value};
use uuid::Uuid;

use minilab_core::EvidenceKind;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

/// Upsert the default claims profile (idempotent by name).
pub async fn ensure_claims_profile(client: &StoreClient, name: &str) -> Result<Uuid, StoreError> {
    let resp = client
        .http
        .post(format!(
            "{}?on_conflict=name",
            client.rest("claims_profile")
        ))
        .header(
            "Prefer",
            "resolution=merge-duplicates,return=representation",
        )
        .json(&json!({ "name": name, "claim_ids": [], "version": 1, "active": true }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let rows: Vec<Value> = resp.json().await?;
    let id = rows
        .into_iter()
        .next()
        .and_then(|r| serde_json::from_value(r["id"].clone()).ok())
        .unwrap_or_else(Uuid::new_v4);

    Ok(id)
}

/// Upsert a campaign (idempotent by name).
pub async fn ensure_campaign(
    client: &StoreClient,
    name: &str,
    channel: &str,
    claims_profile_id: Uuid,
) -> Result<Uuid, StoreError> {
    let resp = client
        .http
        .post(format!("{}?on_conflict=name", client.rest("campaign")))
        .header(
            "Prefer",
            "resolution=merge-duplicates,return=representation",
        )
        .json(&json!({
            "name": name,
            "channel": channel,
            "status": "active",
            "claims_profile_id": claims_profile_id,
            "updated_at": chrono::Utc::now(),
        }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let rows: Vec<Value> = resp.json().await?;
    let id = rows
        .into_iter()
        .next()
        .and_then(|r| serde_json::from_value(r["id"].clone()).ok())
        .unwrap_or_else(Uuid::new_v4);

    Ok(id)
}

fn message_family_evidence_key(campaign_id: Uuid, label: &str) -> String {
    format!("message_family.created:{campaign_id}:{label}")
}

/// Upsert a MessageFamily for the given campaign and write evidence once.
pub async fn create_message_family(
    client: &StoreClient,
    campaign_id: Uuid,
    label: &str,
    template_ref: &str,
    correlation_id: Uuid,
) -> Result<Uuid, StoreError> {
    let resp = client
        .http
        .post(format!(
            "{}?on_conflict=campaign_id,label",
            client.rest("message_family")
        ))
        .header(
            "Prefer",
            "resolution=merge-duplicates,return=representation",
        )
        .json(&json!({
            "campaign_id": campaign_id,
            "label": label,
            "template_ref": template_ref,
            "claims_allowed": [],
        }))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if status >= 300 {
        let text = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let rows: Vec<Value> = resp.json().await?;
    let family_id = rows
        .into_iter()
        .next()
        .and_then(|r| serde_json::from_value(r["id"].clone()).ok())
        .unwrap_or_else(Uuid::new_v4);

    insert_ledger_row(
        client,
        EvidenceKind::MESSAGE_FAMILY_CREATED,
        json!({
            "message_family_id": family_id,
            "campaign_id": campaign_id,
            "label": label,
            "template_ref": template_ref,
        }),
        correlation_id,
        Some(message_family_evidence_key(campaign_id, label)),
    )
    .await?;

    Ok(family_id)
}

#[cfg(test)]
mod tests {
    use super::message_family_evidence_key;
    use uuid::Uuid;

    #[test]
    fn message_family_key_is_deterministic() {
        let campaign_id = Uuid::nil();
        assert_eq!(
            message_family_evidence_key(campaign_id, "angle_pain"),
            "message_family.created:00000000-0000-0000-0000-000000000000:angle_pain"
        );
    }
}
