use minilab_core::evidence::EvidenceKind;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

#[derive(Debug)]
pub enum PolicyDecision {
    Allowed,
    Blocked { reason: String },
}

fn decide_send_policy(
    contact_opt_out: bool,
    account_segment: &str,
    rules: &[Value],
) -> PolicyDecision {
    if contact_opt_out {
        return PolicyDecision::Blocked {
            reason: "contact.opt_out".into(),
        };
    }

    for rule in rules {
        let expr = &rule["expression"];
        let kind = expr["rule_kind"].as_str().unwrap_or("");

        match kind {
            "block_all" => {
                return PolicyDecision::Blocked {
                    reason: format!(
                        "policy_rule:{} block_all",
                        rule["name"].as_str().unwrap_or("?")
                    ),
                };
            }
            "allow_segment" => {
                let required = expr["icp_segment"].as_str().unwrap_or("");
                if account_segment != required {
                    return PolicyDecision::Blocked {
                        reason: format!(
                            "policy_rule:{} allow_segment={required} got={account_segment}",
                            rule["name"].as_str().unwrap_or("?")
                        ),
                    };
                }
            }
            _ => {}
        }
    }

    PolicyDecision::Allowed
}

/// Evaluate send policy for a contact + campaign pair.
///
/// Order of checks (fail-fast):
/// 1. Hard block — contact.opt_out = true
/// 2. Active PolicyRule rows where scope = 'send'
///    Supported rule_kind values (in expression JSON):
///    - `{ "rule_kind": "block_all" }` → always block
///    - `{ "rule_kind": "allow_segment", "icp_segment": "..." }` → block if account doesn't match
pub async fn evaluate_send_policy(
    client: &StoreClient,
    contact_id: Uuid,
    account_id: Uuid,
) -> Result<PolicyDecision, StoreError> {
    let contact_rows: Vec<Value> = client
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

    let contact_opt_out = contact_rows
        .first()
        .and_then(|r| r["opt_out"].as_bool())
        .unwrap_or(false);

    let rules: Vec<Value> = client
        .http
        .get(format!(
            "{}?scope=eq.send&active=eq.true&order=priority.desc",
            client.rest("policy_rule")
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    let acct_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=icp_segment&limit=1",
            client.rest("account"),
            account_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    let segment = acct_rows
        .first()
        .and_then(|r| r["icp_segment"].as_str())
        .unwrap_or("");

    Ok(decide_send_policy(contact_opt_out, segment, &rules))
}

/// Evaluate send policy for the constitutional outbound chain and record
/// `policy.evaluated` evidence in both admissible and blocked cases.
///
/// This is the outbound-orchestrator-facing evaluation. The legacy
/// [`evaluate_send_policy`] remains in place for the sequence-based
/// `send_due` path and continues to write `policy.blocked` via `block_send`
/// on rejection; the two coexist during the transition (see
/// `docs/integration/huge-into-grammar.md` §4).
///
/// Constitutional note: the opt-out dimension is intentionally **not**
/// consulted here. Opt-out is a separate admissibility gate (see
/// [`crate::optout_gate`]) per invariant #3 of the huge memo. This function
/// covers rule-driven policy only. The orchestrator composes both gates.
///
/// Idempotency key: `policy.evaluated:<contact_id>:<campaign_id>:<correlation_id>`.
pub async fn evaluate_and_record_send_policy(
    client: &StoreClient,
    contact_id: Uuid,
    account_id: Uuid,
    campaign_id: Uuid,
    channel: &str,
    correlation_id: Uuid,
) -> Result<PolicyDecision, StoreError> {
    let rules: Vec<Value> = client
        .http
        .get(format!(
            "{}?scope=eq.send&active=eq.true&order=priority.desc",
            client.rest("policy_rule")
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    let acct_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=icp_segment&limit=1",
            client.rest("account"),
            account_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    let segment = acct_rows
        .first()
        .and_then(|r| r["icp_segment"].as_str())
        .unwrap_or("");

    // opt_out is consulted by `optout_gate`; we pass `false` here so this
    // evaluation is pure rule-driven policy.
    let decision = decide_send_policy(false, segment, &rules);

    let rules_matched: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({
                "name": r["name"].as_str().unwrap_or(""),
                "rule_kind": r["expression"]["rule_kind"].as_str().unwrap_or(""),
            })
        })
        .collect();

    let (result_str, reason_detail) = match &decision {
        PolicyDecision::Allowed => ("admissible", Value::Null),
        PolicyDecision::Blocked { reason } => ("blocked", json!(reason)),
    };

    insert_ledger_row(
        client,
        EvidenceKind::POLICY_EVALUATED,
        json!({
            "contact_id": contact_id,
            "account_id": account_id,
            "campaign_id": campaign_id,
            "channel": channel,
            "account_segment": segment,
            "rules_matched": rules_matched,
            "result": result_str,
            "reason_detail": reason_detail,
            "correlation_id": correlation_id,
        }),
        correlation_id,
        Some(format!(
            "policy.evaluated:{contact_id}:{campaign_id}:{correlation_id}"
        )),
    )
    .await?;

    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::{decide_send_policy, PolicyDecision};
    use serde_json::json;

    #[test]
    fn opt_out_blocks_before_rules() {
        let decision = decide_send_policy(
            true,
            "saas_50_500",
            &[json!({
                "name": "allow target",
                "expression": { "rule_kind": "allow_segment", "icp_segment": "saas_50_500" }
            })],
        );
        assert!(
            matches!(decision, PolicyDecision::Blocked { reason } if reason == "contact.opt_out")
        );
    }

    #[test]
    fn block_all_rule_blocks() {
        let decision = decide_send_policy(
            false,
            "saas_50_500",
            &[json!({
                "name": "maintenance",
                "expression": { "rule_kind": "block_all" }
            })],
        );
        assert!(
            matches!(decision, PolicyDecision::Blocked { reason } if reason.contains("block_all"))
        );
    }

    #[test]
    fn allow_segment_rule_blocks_mismatch() {
        let decision = decide_send_policy(
            false,
            "b2b_services",
            &[json!({
                "name": "target-only",
                "expression": { "rule_kind": "allow_segment", "icp_segment": "saas_50_500" }
            })],
        );
        assert!(
            matches!(decision, PolicyDecision::Blocked { reason } if reason.contains("got=b2b_services"))
        );
    }

    #[test]
    fn allow_segment_rule_allows_match() {
        let decision = decide_send_policy(
            false,
            "saas_50_500",
            &[json!({
                "name": "target-only",
                "expression": { "rule_kind": "allow_segment", "icp_segment": "saas_50_500" }
            })],
        );
        assert!(matches!(decision, PolicyDecision::Allowed));
    }

    #[test]
    fn unknown_rules_fall_through() {
        let decision = decide_send_policy(
            false,
            "saas_50_500",
            &[json!({
                "name": "future-rule",
                "expression": { "rule_kind": "future_kind" }
            })],
        );
        assert!(matches!(decision, PolicyDecision::Allowed));
    }
}
