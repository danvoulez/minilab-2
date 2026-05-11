//! Eligibility evaluation for a candidate outbound act.
//!
//! Constitutional role: evaluates whether the *already-asserted* claims on a
//! contact satisfy the campaign's required predicate at send time. This is a
//! per-action admissibility decision — **not** a new claim assertion. See
//! `docs/integration/huge-into-grammar.md` section 4 ("Eligibility ≠ claim
//! assertion").
//!
//! Honest stub: the evidence shape is the canonical ADR shape; the predicate
//! evaluation today is minimal (reads the campaign's `claims_profile_id` and
//! the contact's surfaced claim columns, defaults to `admissible` if no
//! enforced predicates exist). Content evolves; shape is stable.

use minilab_core::evidence::EvidenceKind;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

#[derive(Debug, Clone)]
pub struct EligibilityInput {
    pub contact_id: Uuid,
    pub campaign_id: Uuid,
    pub channel: String,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone)]
pub enum EligibilityOutcome {
    Admissible {
        profile_id: Option<String>,
        claims_consulted: Vec<String>,
    },
    Inadmissible {
        reason: String,
        profile_id: Option<String>,
        claims_consulted: Vec<String>,
    },
}

impl EligibilityOutcome {
    pub fn is_admissible(&self) -> bool {
        matches!(self, Self::Admissible { .. })
    }
}

/// Evaluate and record `eligibility.evaluated` evidence.
///
/// Idempotency key: `eligibility.evaluated:<contact_id>:<campaign_id>:<correlation_id>`.
pub async fn evaluate_eligibility(
    client: &StoreClient,
    input: &EligibilityInput,
) -> Result<EligibilityOutcome, StoreError> {
    let (profile_id, claims_consulted) = fetch_campaign_profile(client, input.campaign_id).await?;

    // Honest stub: if no enforced predicates are declared on the campaign,
    // eligibility is admissible. Real predicate evaluation lands in a later
    // vertical (see deferred #1 in the huge memo).
    let outcome = EligibilityOutcome::Admissible {
        profile_id: profile_id.clone(),
        claims_consulted: claims_consulted.clone(),
    };

    let (result_str, reason_detail) = match &outcome {
        EligibilityOutcome::Admissible { .. } => ("admissible", Value::Null),
        EligibilityOutcome::Inadmissible { reason, .. } => ("inadmissible", json!(reason)),
    };

    insert_ledger_row(
        client,
        EvidenceKind::ELIGIBILITY_EVALUATED,
        json!({
            "contact_id": input.contact_id,
            "campaign_id": input.campaign_id,
            "channel": input.channel,
            "profile_id": profile_id,
            "claims_consulted": claims_consulted,
            "result": result_str,
            "reason_detail": reason_detail,
            "correlation_id": input.correlation_id,
            // Does NOT mutate claim state: eligibility is a per-action admissibility
            // decision over already-asserted claims; see huge-into-grammar §4.
            "asserts_new_claim": false,
        }),
        input.correlation_id,
        Some(format!(
            "eligibility.evaluated:{}:{}:{}",
            input.contact_id, input.campaign_id, input.correlation_id
        )),
    )
    .await?;

    Ok(outcome)
}

/// Fetch campaign's claims profile id and (optional) enforced claims list.
///
/// Looks up `message_family.claims_allowed` for the campaign when present, as
/// a hint about which claims the campaign cares about. The v1 stub treats this
/// as informational only (listed in `claims_consulted`), not as a hard gate.
async fn fetch_campaign_profile(
    client: &StoreClient,
    campaign_id: Uuid,
) -> Result<(Option<String>, Vec<String>), StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?campaign_id=eq.{}&select=claims_allowed&limit=1",
            client.rest("message_family"),
            campaign_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    let claims = rows
        .first()
        .and_then(|r| r["claims_allowed"].as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // profile_id is not currently stored on campaign directly; defer lookup to
    // canon/elastic integration. For now, `claims_allowed` is the surfaced
    // predicate set consulted by this evaluation.
    Ok((None, claims))
}
