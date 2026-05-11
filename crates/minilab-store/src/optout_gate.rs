//! Opt-out admissibility gate consulted during outbound lowering.
//!
//! Constitutional role: invariant #3 of `docs/integration/huge-into-grammar.md`
//! — "Opt-out is a lowering-time gate." An outbound intent against a contact
//! with an active opt-out covering the candidate channel is rejected *before*
//! the command is lowered, not at the provider boundary.
//!
//! This module is **read-side only**. It never asserts opt-out. Opt-out is
//! asserted during reply ingestion (see `reply::apply_opt_out`) and persists
//! as `optout.asserted` evidence (canonical) alongside the legacy
//! `contact.opt_out = true` column (compat). Both are consulted here during
//! the transition.
//!
//! No `.evaluated` evidence is emitted by this gate: the gate reads existing
//! assertions rather than producing a new evaluation artifact. Blocking
//! outcomes surface through the orchestrator as `outbound.failed` with
//! `optout_active` reason.

use serde_json::Value;
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};

#[derive(Debug, Clone)]
pub struct OptOutInput {
    pub contact_id: Uuid,
    pub channel: String,
}

#[derive(Debug, Clone)]
pub enum OptOutOutcome {
    Clear,
    Asserted {
        /// Where the assertion was observed: `contact_column`, `ledger`, or both.
        source: &'static str,
        /// Free-form detail (e.g. reply classification correlation_id).
        detail: Option<String>,
    },
}

impl OptOutOutcome {
    pub fn is_clear(&self) -> bool {
        matches!(self, Self::Clear)
    }
}

/// Evaluate whether the contact has an active opt-out covering the candidate
/// channel.
///
/// Consults both:
/// 1. `contact.opt_out = true` (legacy column, remains authoritative for the
///    existing sequence flow during transition).
/// 2. `evidence_ledger` rows of kind `optout.asserted` for this contact
///    (canonical per huge-into-grammar §2).
///
/// Either being present blocks the send. When both appear, `source` is
/// reported as `both` so downstream audit can see the transition state.
pub async fn evaluate_optout_gate(
    client: &StoreClient,
    input: &OptOutInput,
) -> Result<OptOutOutcome, StoreError> {
    let legacy_asserted = read_contact_opt_out(client, input.contact_id).await?;
    let ledger_detail = read_ledger_optout(client, input.contact_id, &input.channel).await?;

    match (legacy_asserted, ledger_detail) {
        (false, None) => Ok(OptOutOutcome::Clear),
        (true, None) => Ok(OptOutOutcome::Asserted {
            source: "contact_column",
            detail: None,
        }),
        (false, Some(detail)) => Ok(OptOutOutcome::Asserted {
            source: "ledger",
            detail: Some(detail),
        }),
        (true, Some(detail)) => Ok(OptOutOutcome::Asserted {
            source: "both",
            detail: Some(detail),
        }),
    }
}

async fn read_contact_opt_out(client: &StoreClient, contact_id: Uuid) -> Result<bool, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=opt_out&limit=1",
            client.rest("contact"),
            contact_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    Ok(rows
        .first()
        .and_then(|r| r["opt_out"].as_bool())
        .unwrap_or(false))
}

/// Returns the correlation_id of the latest covering `optout.asserted` row,
/// if any. A row "covers" the candidate channel when its payload `channel`
/// matches the input channel or equals `"*"` (global opt-out).
async fn read_ledger_optout(
    client: &StoreClient,
    contact_id: Uuid,
    channel: &str,
) -> Result<Option<String>, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?kind=eq.optout.asserted&order=created_at.desc&limit=20",
            client.rest("evidence_ledger")
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    for row in rows {
        let payload = &row["payload"];
        let row_contact = payload["contact_id"].as_str().unwrap_or("");
        if row_contact != contact_id.to_string() {
            continue;
        }
        let row_channel = payload["channel"].as_str().unwrap_or("");
        if row_channel == channel || row_channel == "*" {
            return Ok(payload["correlation_id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| row["correlation_id"].as_str().map(str::to_owned)));
        }
    }
    Ok(None)
}
