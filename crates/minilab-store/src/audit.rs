use chrono::{DateTime, Utc};
use minilab_core::{EvidenceKind, SimMode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};

/// A row read from the append-only business evidence ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceTrailRow {
    pub id: Uuid,
    pub kind: String,
    pub payload: Value,
    pub correlation_id: Uuid,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub sim_mode: SimMode,
    pub idempotency_key: Option<String>,
}

/// Replay-oriented export for one correlation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceTrail {
    pub correlation_id: Uuid,
    pub rows: Vec<EvidenceTrailRow>,
    pub summary: EvidenceTrailSummary,
    pub explainability: TouchExplanation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceTrailSummary {
    pub total_rows: usize,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub kinds: Vec<String>,
    pub evidence_closed: bool,
    pub missing_expected_kinds: Vec<String>,
    pub terminal_kind: Option<String>,
    pub production_rows: usize,
    pub non_production_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TouchExplanation {
    pub correlation_id: Uuid,
    pub contacted: bool,
    pub blocked: bool,
    pub block_reasons: Vec<String>,
    pub why_contacted: Vec<String>,
    pub message_family: Option<String>,
    pub channel: Option<String>,
    pub provider_message_id: Option<String>,
    pub reply_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KpiSnapshot {
    pub total_correlations: usize,
    pub correlations_with_send: usize,
    pub correlations_blocked: usize,
    pub correlations_with_reply: usize,
    pub positive_replies: usize,
    pub premium_exceptions: usize,
    pub total_evidence_rows: usize,
    pub non_production_rows: usize,
}

impl KpiSnapshot {
    pub fn autonomy_rate(&self) -> f64 {
        if self.total_correlations == 0 {
            return 0.0;
        }
        let escalated_or_blocked = self.premium_exceptions + self.correlations_blocked;
        self.total_correlations.saturating_sub(escalated_or_blocked) as f64
            / self.total_correlations as f64
    }

    pub fn evidence_closure_rate(&self) -> f64 {
        if self.total_correlations == 0 {
            return 0.0;
        }
        self.correlations_with_send as f64 / self.total_correlations as f64
    }

    pub fn positive_reply_rate(&self) -> f64 {
        if self.correlations_with_reply == 0 {
            return 0.0;
        }
        self.positive_replies as f64 / self.correlations_with_reply as f64
    }
}

/// Fetch, sort, summarize, and explain all evidence for one correlation.
pub async fn export_evidence_trail(
    client: &StoreClient,
    correlation_id: Uuid,
) -> Result<EvidenceTrail, StoreError> {
    let rows = fetch_evidence_rows(client, correlation_id).await?;
    Ok(build_evidence_trail(correlation_id, rows))
}

pub async fn fetch_evidence_rows(
    client: &StoreClient,
    correlation_id: Uuid,
) -> Result<Vec<EvidenceTrailRow>, StoreError> {
    let resp = client
        .http
        .get(client.rest("evidence_ledger"))
        .query(&[
            (
                "select",
                "id,kind,payload,correlation_id,created_at,sim_mode,idempotency_key".to_string(),
            ),
            ("correlation_id", format!("eq.{correlation_id}")),
            ("order", "created_at.asc,id.asc".to_string()),
        ])
        .send()
        .await?;

    let status = resp.status().as_u16();
    if status >= 300 {
        let body = resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body });
    }

    resp.json::<Vec<EvidenceTrailRow>>()
        .await
        .map_err(StoreError::from)
}

pub fn build_evidence_trail(
    correlation_id: Uuid,
    mut rows: Vec<EvidenceTrailRow>,
) -> EvidenceTrail {
    rows.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let summary = summarize_evidence_rows(&rows);
    let explainability = explain_touch(correlation_id, &rows);
    EvidenceTrail {
        correlation_id,
        rows,
        summary,
        explainability,
    }
}

pub fn summarize_evidence_rows(rows: &[EvidenceTrailRow]) -> EvidenceTrailSummary {
    let mut kinds = Vec::new();
    let mut production_rows = 0;
    let mut non_production_rows = 0;
    for row in rows {
        if !kinds.contains(&row.kind) {
            kinds.push(row.kind.clone());
        }
        if row.sim_mode.is_production() {
            production_rows += 1;
        } else {
            non_production_rows += 1;
        }
    }

    let terminal_kind = rows
        .iter()
        .rev()
        .find(|row| is_terminal_kind(&row.kind))
        .map(|row| row.kind.clone());
    let missing_expected_kinds = expected_gtm_kinds(rows)
        .into_iter()
        .filter(|kind| !rows.iter().any(|row| row.kind == *kind))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let evidence_closed = terminal_kind.is_some() && missing_expected_kinds.is_empty();

    EvidenceTrailSummary {
        total_rows: rows.len(),
        first_seen_at: rows.first().map(|row| row.created_at),
        last_seen_at: rows.last().map(|row| row.created_at),
        kinds,
        evidence_closed,
        missing_expected_kinds,
        terminal_kind,
        production_rows,
        non_production_rows,
    }
}

pub fn explain_touch(correlation_id: Uuid, rows: &[EvidenceTrailRow]) -> TouchExplanation {
    let contacted = rows.iter().any(|row| {
        matches!(
            row.kind.as_str(),
            EvidenceKind::OUTREACH_SENT | EvidenceKind::OUTBOUND_SENT
        )
    });
    let blocked = rows.iter().any(|row| {
        matches!(
            row.kind.as_str(),
            EvidenceKind::POLICY_BLOCKED | EvidenceKind::OUTBOUND_FAILED
        )
    });

    let mut why_contacted = Vec::new();
    let mut block_reasons = Vec::new();
    let mut message_family = None;
    let mut channel = None;
    let mut provider_message_id = None;
    let mut reply_category = None;

    for row in rows {
        match row.kind.as_str() {
            EvidenceKind::TRIGGER_INGESTED | EvidenceKind::BUYING_SIGNAL_OBSERVED => {
                push_unique(&mut why_contacted, "buying signal was recorded");
            }
            EvidenceKind::ACCOUNT_SCORED => {
                let score = row.payload.get("score").and_then(Value::as_i64);
                if let Some(score) = score {
                    push_unique(&mut why_contacted, &format!("account score was {score}"));
                } else {
                    push_unique(&mut why_contacted, "account was scored");
                }
            }
            EvidenceKind::MESSAGE_FAMILY_CREATED => {
                message_family = first_string(
                    &row.payload,
                    &["message_family_id", "family_id", "message_family", "name"],
                )
                .or(message_family);
                push_unique(&mut why_contacted, "approved message family was available");
            }
            EvidenceKind::ELIGIBILITY_EVALUATED
                if payload_bool(&row.payload, &["eligible", "allowed"]) == Some(true) =>
            {
                push_unique(&mut why_contacted, "eligibility gate allowed the contact");
            }
            EvidenceKind::PREMIUM_EVALUATED
                if payload_bool(&row.payload, &["premium_required", "requires_premium"])
                    == Some(false) =>
            {
                push_unique(&mut why_contacted, "premium gate stayed routine");
            }
            EvidenceKind::POLICY_EVALUATED
                if payload_bool(&row.payload, &["allowed", "pass"]) == Some(true) =>
            {
                push_unique(&mut why_contacted, "policy gate allowed the send");
            }
            EvidenceKind::OUTBOUND_ADMITTED => {
                push_unique(&mut why_contacted, "outbound act was admitted");
            }
            EvidenceKind::OUTREACH_SENT | EvidenceKind::OUTBOUND_SENT => {
                channel = first_string(&row.payload, &["channel"]).or(channel);
                provider_message_id = first_string(
                    &row.payload,
                    &["provider_message_id", "message_id", "channel_message_id"],
                )
                .or(provider_message_id);
            }
            EvidenceKind::POLICY_BLOCKED | EvidenceKind::OUTBOUND_FAILED => {
                if let Some(reason) =
                    first_string(&row.payload, &["reason", "block_reason", "error"])
                {
                    push_unique(&mut block_reasons, &reason);
                } else {
                    push_unique(&mut block_reasons, row.kind.as_str());
                }
            }
            EvidenceKind::REPLY_CLASSIFIED => {
                reply_category =
                    first_string(&row.payload, &["category", "reply_category"]).or(reply_category);
            }
            _ => {}
        }
    }

    TouchExplanation {
        correlation_id,
        contacted,
        blocked,
        block_reasons,
        why_contacted,
        message_family,
        channel,
        provider_message_id,
        reply_category,
    }
}

pub fn kpi_snapshot(trails: &[EvidenceTrail]) -> KpiSnapshot {
    let mut correlations_with_send = 0;
    let mut correlations_blocked = 0;
    let mut correlations_with_reply = 0;
    let mut positive_replies = 0;
    let mut premium_exceptions = 0;
    let mut total_evidence_rows = 0;
    let mut non_production_rows = 0;

    for trail in trails {
        total_evidence_rows += trail.rows.len();
        non_production_rows += trail.summary.non_production_rows;
        if trail.explainability.contacted {
            correlations_with_send += 1;
        }
        if trail.explainability.blocked {
            correlations_blocked += 1;
        }
        if trail.explainability.reply_category.is_some() {
            correlations_with_reply += 1;
        }
        if matches!(
            trail.explainability.reply_category.as_deref(),
            Some("positive")
        ) {
            positive_replies += 1;
        }
        if trail
            .rows
            .iter()
            .any(|row| row.kind == EvidenceKind::PREMIUM_EXCEPTION_RAISED)
        {
            premium_exceptions += 1;
        }
    }

    KpiSnapshot {
        total_correlations: trails.len(),
        correlations_with_send,
        correlations_blocked,
        correlations_with_reply,
        positive_replies,
        premium_exceptions,
        total_evidence_rows,
        non_production_rows,
    }
}

pub fn trail_json(trail: &EvidenceTrail) -> Value {
    json!({
        "correlation_id": trail.correlation_id,
        "summary": trail.summary,
        "explainability": trail.explainability,
        "rows": trail.rows,
    })
}

fn expected_gtm_kinds(rows: &[EvidenceTrailRow]) -> Vec<&'static str> {
    let outbound_slice = rows.iter().any(|row| {
        matches!(
            row.kind.as_str(),
            EvidenceKind::OUTBOUND_SENT
                | EvidenceKind::OUTBOUND_FAILED
                | EvidenceKind::OUTBOUND_ADMITTED
        )
    });
    if outbound_slice {
        return vec![
            EvidenceKind::ELIGIBILITY_EVALUATED,
            EvidenceKind::PREMIUM_EVALUATED,
            EvidenceKind::POLICY_EVALUATED,
            EvidenceKind::OUTBOUND_ADMITTED,
        ];
    }

    let week1 = rows.iter().any(|row| {
        matches!(
            row.kind.as_str(),
            EvidenceKind::TRIGGER_INGESTED
                | EvidenceKind::ACCOUNT_CREATED
                | EvidenceKind::ACCOUNT_MERGED
                | EvidenceKind::ACCOUNT_SCORED
                | EvidenceKind::MESSAGE_FAMILY_CREATED
                | EvidenceKind::OUTREACH_SENT
        )
    });
    if week1 {
        return vec![
            EvidenceKind::TRIGGER_INGESTED,
            EvidenceKind::ACCOUNT_SCORED,
            EvidenceKind::MESSAGE_FAMILY_CREATED,
        ];
    }

    Vec::new()
}

fn is_terminal_kind(kind: &str) -> bool {
    matches!(
        kind,
        EvidenceKind::OUTREACH_SENT
            | EvidenceKind::OUTBOUND_SENT
            | EvidenceKind::OUTBOUND_FAILED
            | EvidenceKind::POLICY_BLOCKED
            | EvidenceKind::REPLY_CLASSIFIED
            | EvidenceKind::CONTACT_OPTED_OUT
            | EvidenceKind::MEETING_SCHEDULED
            | EvidenceKind::MEETING_COMPLETED
    )
}

fn first_string(payload: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = payload.get(*key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn payload_bool(payload: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_bool))
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, payload: Value, offset: i64) -> EvidenceTrailRow {
        EvidenceTrailRow {
            id: Uuid::from_u128(offset as u128 + 1),
            kind: kind.into(),
            payload,
            correlation_id: Uuid::nil(),
            created_at: DateTime::from_timestamp(1_700_000_000 + offset, 0).unwrap(),
            sim_mode: SimMode::Production,
            idempotency_key: None,
        }
    }

    #[test]
    fn summarizes_closed_outbound_slice() {
        let rows = vec![
            row(
                EvidenceKind::ELIGIBILITY_EVALUATED,
                json!({"eligible": true}),
                0,
            ),
            row(
                EvidenceKind::PREMIUM_EVALUATED,
                json!({"premium_required": false}),
                1,
            ),
            row(EvidenceKind::POLICY_EVALUATED, json!({"allowed": true}), 2),
            row(EvidenceKind::OUTBOUND_ADMITTED, json!({}), 3),
            row(
                EvidenceKind::OUTBOUND_SENT,
                json!({"provider_message_id":"sg_1","channel":"email"}),
                4,
            ),
        ];
        let trail = build_evidence_trail(Uuid::nil(), rows);
        assert!(trail.summary.evidence_closed);
        assert_eq!(
            trail.summary.terminal_kind.as_deref(),
            Some(EvidenceKind::OUTBOUND_SENT)
        );
        assert_eq!(
            trail.explainability.provider_message_id.as_deref(),
            Some("sg_1")
        );
        assert!(trail
            .explainability
            .why_contacted
            .contains(&"policy gate allowed the send".to_string()));
    }

    #[test]
    fn detects_missing_expected_kinds() {
        let rows = vec![row(EvidenceKind::OUTBOUND_SENT, json!({}), 0)];
        let summary = summarize_evidence_rows(&rows);
        assert!(!summary.evidence_closed);
        assert!(summary
            .missing_expected_kinds
            .contains(&EvidenceKind::POLICY_EVALUATED.to_string()));
    }

    #[test]
    fn kpi_snapshot_counts_rates() {
        let sent = build_evidence_trail(
            Uuid::nil(),
            vec![
                row(EvidenceKind::OUTBOUND_SENT, json!({}), 0),
                row(
                    EvidenceKind::REPLY_CLASSIFIED,
                    json!({"category":"positive"}),
                    1,
                ),
            ],
        );
        let blocked = build_evidence_trail(
            Uuid::from_u128(2),
            vec![row(
                EvidenceKind::POLICY_BLOCKED,
                json!({"reason":"opt_out"}),
                0,
            )],
        );
        let snapshot = kpi_snapshot(&[sent, blocked]);
        assert_eq!(snapshot.total_correlations, 2);
        assert_eq!(snapshot.correlations_with_send, 1);
        assert_eq!(snapshot.correlations_blocked, 1);
        assert_eq!(snapshot.positive_reply_rate(), 1.0);
        assert_eq!(snapshot.evidence_closure_rate(), 0.5);
    }
}
