//! Premium policy evaluation for a candidate outbound act.
//!
//! Constitutional role: a predicate evaluated over a candidate `Execute(outbound.*)`
//! whose result must be recorded in the same lineage before lowering makes
//! the execution admissible. See `docs/integration/huge-into-grammar.md`
//! invariant #2.
//!
//! Honest stub: the evidence shape carries `allowed_reason` and `bounds` per
//! the ADR; the current evaluation defaults to admissible with a
//! `default_bounds` reason. Real premium evaluation (bounds widening,
//! allowed-reason catalog, Compliance envelope) lands in a later vertical.

use minilab_core::evidence::EvidenceKind;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

#[derive(Debug, Clone)]
pub struct PremiumInput {
    pub contact_id: Uuid,
    pub campaign_id: Uuid,
    pub channel: String,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone)]
pub enum PremiumOutcome {
    Admissible { allowed_reason: String },
    Blocked { reason: String },
}

impl PremiumOutcome {
    pub fn is_admissible(&self) -> bool {
        matches!(self, Self::Admissible { .. })
    }
}

/// Evaluate and record `premium.evaluated` evidence.
///
/// Idempotency key: `premium.evaluated:<contact_id>:<campaign_id>:<correlation_id>`.
pub async fn evaluate_premium(
    client: &StoreClient,
    input: &PremiumInput,
) -> Result<PremiumOutcome, StoreError> {
    // Honest stub: until the canon-declared premium policy is wired, outbound
    // under the already-approved channel set defaults to admissible under
    // `default_bounds`. A widening beyond these bounds is defined to require
    // Compliance re-authorization (deferred #7 in the huge memo).
    let outcome = PremiumOutcome::Admissible {
        allowed_reason: "default_bounds".into(),
    };

    let (result_str, allowed_reason, reason_detail) = match &outcome {
        PremiumOutcome::Admissible { allowed_reason } => {
            ("admissible", json!(allowed_reason), Value::Null)
        }
        PremiumOutcome::Blocked { reason } => ("blocked", Value::Null, json!(reason)),
    };

    insert_ledger_row(
        client,
        EvidenceKind::PREMIUM_EVALUATED,
        json!({
            "contact_id": input.contact_id,
            "campaign_id": input.campaign_id,
            "channel": input.channel,
            "result": result_str,
            "allowed_reason": allowed_reason,
            "reason_detail": reason_detail,
            "bounds": "default_bounds",
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "premium.evaluated:{}:{}:{}",
            input.contact_id, input.campaign_id, input.correlation_id
        )),
    )
    .await?;

    Ok(outcome)
}
