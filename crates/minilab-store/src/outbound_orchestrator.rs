//! Constitutional outbound.send orchestrator.
//!
//! Composes the admissibility chain for a single outbound act, per
//! `docs/integration/huge-into-grammar.md` §4. Every run produces a
//! constitutional lineage in the evidence ledger, under a single
//! `correlation_id`:
//!
//! ```text
//!   eligibility.evaluated
//!   premium.evaluated
//!   policy.evaluated
//!   outbound.admitted          (marks the admissibility barrier crossing)
//!   outbound.sent | outbound.failed
//! ```
//!
//! Each step is distinct semantically even where implementations share code:
//! the opt-out gate is a separate evaluation from policy rules (invariant #3
//! of the huge memo). `outbound.failed` is the kind emitted for lowering-time
//! admissibility rejection of an outbound intent — not merely
//! provider-boundary failure (see huge-into-grammar §4 failure path note).
//!
//! Input is deliberately minimal: `{ contact_id, campaign_id, channel,
//! correlation_id }`. `account_id` and downstream dispatch identifiers are
//! resolved internally so external callers cannot inject pseudo-authoritative
//! context.
//!
//! # Design memory (slice decisions, April 2026)
//!
//! These are deliberate and should not be "fixed" by future passes without
//! re-opening the ADR:
//!
//! - **IR-level `outbound.send` lowerer and dispatcher both landed.** The
//!   `MinilabRuntimeLowerer` recognizes
//!   `ActionKind::Canonical(CanonicalActionId("outbound.send"))`
//!   and emits `OperationalCommand { namespace: "outbound", verb: "send",
//!   target_runtime: Provider }` with the full success-path evidence
//!   contract. The live HTTP path now goes through
//!   `lower_and_dispatch_execute` -> `dispatch_operational_command` before it
//!   reaches this orchestrator. This module remains the composer of the
//!   constitutional chain; the dispatcher is only the canonical handoff from
//!   lowered command to typed slice input.
//! - **Coexistence with the legacy sequence flow is intentional.** This path
//!   does not write `outreach.sent` or `policy.blocked`; those remain
//!   authoritative for the sequence-based `bin/send_due` flow during the
//!   transition. Unification is a future vertical — not this one.
//! - **Eligibility and premium are honest stubs.** The evidence shapes are
//!   the canonical ADR shapes; the predicate content is minimal. Real
//!   predicate evaluation (claim profile gating, premium bound-widening) is
//!   deferred per the memo, but the shape is stable — new content plugs in
//!   without reshaping the ledger.

use minilab_core::evidence::EvidenceKind;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::eligibility::{evaluate_eligibility, EligibilityInput, EligibilityOutcome};
use crate::evidence::insert_ledger_row;
use crate::optout_gate::{evaluate_optout_gate, OptOutInput, OptOutOutcome};
use crate::outbound::{dispatch_sequence, DeliveryReceipt};
use crate::policy::{evaluate_and_record_send_policy, PolicyDecision};
use crate::premium::{evaluate_premium, PremiumInput, PremiumOutcome};

/// Minimal input for the constitutional outbound.send chain.
#[derive(Debug, Clone)]
pub struct OutboundSendInput {
    pub contact_id: Uuid,
    pub campaign_id: Uuid,
    pub channel: String,
    pub correlation_id: Uuid,
}

/// Terminal outcome of [`submit_outbound_send`]. Every branch has closed
/// evidence — no silent drop path exists.
#[derive(Debug, Clone)]
pub enum OutboundSendOutcome {
    /// Provider accepted the message; `outbound.sent` is written.
    Sent {
        provider: &'static str,
        provider_message_id: String,
        channel: String,
    },
    /// Rejected during the admissibility chain (pre-lowering). `outbound.failed`
    /// is written with the corresponding `reason_code`.
    Rejected { reason_code: String, detail: String },
    /// Provider boundary error after admission. `outbound.failed` is written
    /// with `provider_rejected` / `provider_error`.
    ProviderError { reason_code: String, detail: String },
}

impl OutboundSendOutcome {
    pub fn was_sent(&self) -> bool {
        matches!(self, Self::Sent { .. })
    }
}

/// Run the full admissibility chain and, if admitted, dispatch via the
/// existing provider path.
///
/// Every terminal branch writes closed evidence. The chain is:
///
/// 1. Resolve `account_id` from the contact.
/// 2. `eligibility.evaluated`
/// 3. `premium.evaluated`
/// 4. opt-out gate (reads existing assertions; does not write)
/// 5. `policy.evaluated`
/// 6. `outbound.admitted`  → dispatch → `outbound.sent`
///    or any block → `outbound.failed`.
pub async fn submit_outbound_send(
    client: &StoreClient,
    input: OutboundSendInput,
) -> Result<OutboundSendOutcome, StoreError> {
    // --- 1. Resolve contact → account ---------------------------------------
    let account_id = match resolve_account_for_contact(client, input.contact_id).await? {
        Some(id) => id,
        None => {
            return reject(
                client,
                &input,
                "contact_suppressed",
                "contact not found or suppressed",
                None,
            )
            .await;
        }
    };

    // --- 2. Eligibility ------------------------------------------------------
    let eligibility = evaluate_eligibility(
        client,
        &EligibilityInput {
            contact_id: input.contact_id,
            campaign_id: input.campaign_id,
            channel: input.channel.clone(),
            correlation_id: input.correlation_id,
        },
    )
    .await?;
    if let EligibilityOutcome::Inadmissible { reason, .. } = &eligibility {
        return reject(client, &input, "claim_ineligible", reason, Some(account_id)).await;
    }

    // --- 3. Premium ----------------------------------------------------------
    let premium = evaluate_premium(
        client,
        &PremiumInput {
            contact_id: input.contact_id,
            campaign_id: input.campaign_id,
            channel: input.channel.clone(),
            correlation_id: input.correlation_id,
        },
    )
    .await?;
    if let PremiumOutcome::Blocked { reason } = &premium {
        return reject(client, &input, "premium_blocked", reason, Some(account_id)).await;
    }

    // --- 4. Opt-out gate (semantic boundary separate from rule policy) -------
    let optout = evaluate_optout_gate(
        client,
        &OptOutInput {
            contact_id: input.contact_id,
            channel: input.channel.clone(),
        },
    )
    .await?;
    if let OptOutOutcome::Asserted { source, detail } = &optout {
        let detail_str = format!(
            "opt-out observed via {source}{}",
            detail
                .as_deref()
                .map(|d| format!(" (lineage={d})"))
                .unwrap_or_default()
        );
        return reject(
            client,
            &input,
            "optout_active",
            &detail_str,
            Some(account_id),
        )
        .await;
    }

    // --- 5. Policy rules -----------------------------------------------------
    let policy = evaluate_and_record_send_policy(
        client,
        input.contact_id,
        account_id,
        input.campaign_id,
        &input.channel,
        input.correlation_id,
    )
    .await?;
    if let PolicyDecision::Blocked { reason } = &policy {
        return reject(client, &input, "policy_blocked", reason, Some(account_id)).await;
    }

    // --- 6. Admission + dispatch --------------------------------------------
    // `outbound.admitted` marks the explicit crossing of the constitutional
    // admissibility barrier. It separates "evaluated and admissible" from
    // "actually sent" — invaluable for replay, simulation diffs, and failure
    // archaeology.
    let dispatch_id = Uuid::new_v4();
    insert_ledger_row(
        client,
        EvidenceKind::OUTBOUND_ADMITTED,
        json!({
            "contact_id": input.contact_id,
            "account_id": account_id,
            "campaign_id": input.campaign_id,
            "channel": input.channel,
            "dispatch_id": dispatch_id,
            "correlation_id": input.correlation_id,
            "sim_mode": format!("{:?}", client.sim_mode).to_ascii_lowercase(),
        }),
        input.correlation_id,
        Some(format!(
            "outbound.admitted:{}:{}:{}",
            input.campaign_id, input.contact_id, input.correlation_id
        )),
    )
    .await?;

    match dispatch_sequence(
        client,
        dispatch_id,
        account_id,
        input.contact_id,
        input.campaign_id,
        input.correlation_id,
    )
    .await
    {
        Ok(receipt) => record_sent(client, &input, account_id, dispatch_id, receipt).await,
        Err(err) => match err {
            StoreError::Provider {
                provider,
                status,
                body,
            } => {
                reject_after_admission(
                    client,
                    &input,
                    account_id,
                    dispatch_id,
                    "provider_rejected",
                    &format!("{provider} status {status}: {body}"),
                )
                .await
            }
            StoreError::SendBlocked { mode } => {
                // Config drift: sim_mode blocks real sends but an explicit
                // non-mock provider was configured. Surface as a provider-class
                // failure with a distinctive reason.
                reject_after_admission(
                    client,
                    &input,
                    account_id,
                    dispatch_id,
                    "provider_error",
                    &format!("send blocked by sim_mode={mode:?}"),
                )
                .await
            }
            other => Err(other),
        },
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

async fn resolve_account_for_contact(
    client: &StoreClient,
    contact_id: Uuid,
) -> Result<Option<Uuid>, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=account_id&limit=1",
            client.rest("contact"),
            contact_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    Ok(rows
        .first()
        .and_then(|r| r["account_id"].as_str())
        .and_then(|s| Uuid::parse_str(s).ok()))
}

async fn record_sent(
    client: &StoreClient,
    input: &OutboundSendInput,
    account_id: Uuid,
    dispatch_id: Uuid,
    receipt: DeliveryReceipt,
) -> Result<OutboundSendOutcome, StoreError> {
    insert_ledger_row(
        client,
        EvidenceKind::OUTBOUND_SENT,
        json!({
            "contact_id": input.contact_id,
            "account_id": account_id,
            "campaign_id": input.campaign_id,
            "channel": receipt.channel,
            "provider": receipt.provider,
            "provider_message_id": receipt.provider_message_id,
            "template_ref": receipt.template_ref,
            "message_family_id": receipt.message_family_id,
            "dispatch_id": dispatch_id,
            "mock": receipt.mock,
            "operational_command": {
                "namespace": receipt.operational_command.namespace,
                "verb": receipt.operational_command.verb,
                "target_runtime": receipt.operational_command.target_runtime,
            },
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!("outbound.sent:{}", receipt.provider_message_id)),
    )
    .await?;

    Ok(OutboundSendOutcome::Sent {
        provider: receipt.provider,
        provider_message_id: receipt.provider_message_id,
        channel: receipt.channel,
    })
}

/// Pre-lowering admissibility rejection. Writes `outbound.failed` with the
/// supplied `reason_code`. `outbound.failed` intentionally covers both
/// pre-execution admissibility rejection and post-admission provider failure;
/// the operational story (caller sees one failure kind) remains coherent. See
/// huge-into-grammar §4.
async fn reject(
    client: &StoreClient,
    input: &OutboundSendInput,
    reason_code: &str,
    reason_detail: &str,
    account_id: Option<Uuid>,
) -> Result<OutboundSendOutcome, StoreError> {
    insert_ledger_row(
        client,
        EvidenceKind::OUTBOUND_FAILED,
        json!({
            "contact_id": input.contact_id,
            "account_id": account_id,
            "campaign_id": input.campaign_id,
            "channel": input.channel,
            "reason_code": reason_code,
            "reason_detail": reason_detail,
            "phase": "pre_admission",
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "outbound.failed:{}:{}:{}:{}",
            input.campaign_id, input.contact_id, input.correlation_id, reason_code
        )),
    )
    .await?;

    Ok(OutboundSendOutcome::Rejected {
        reason_code: reason_code.into(),
        detail: reason_detail.into(),
    })
}

async fn reject_after_admission(
    client: &StoreClient,
    input: &OutboundSendInput,
    account_id: Uuid,
    dispatch_id: Uuid,
    reason_code: &str,
    reason_detail: &str,
) -> Result<OutboundSendOutcome, StoreError> {
    insert_ledger_row(
        client,
        EvidenceKind::OUTBOUND_FAILED,
        json!({
            "contact_id": input.contact_id,
            "account_id": account_id,
            "campaign_id": input.campaign_id,
            "channel": input.channel,
            "dispatch_id": dispatch_id,
            "reason_code": reason_code,
            "reason_detail": reason_detail,
            "phase": "post_admission",
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "outbound.failed:{}:{}:{}:{}",
            input.campaign_id, input.contact_id, input.correlation_id, reason_code
        )),
    )
    .await?;

    Ok(OutboundSendOutcome::ProviderError {
        reason_code: reason_code.into(),
        detail: reason_detail.into(),
    })
}
