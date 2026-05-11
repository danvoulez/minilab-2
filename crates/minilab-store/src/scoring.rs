use serde_json::{json, Value};
use uuid::Uuid;

use minilab_core::EvidenceKind;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

/// Rule-based L1 scorer. Returns the new score (0–100).
///
/// Rules (additive):
///  +50  base
///  +20  icp_segment is a known target segment
///  +15  has a job_post trigger in the last 30 days
///  +10  has more than one trigger event (repeat signal)
///  −20  strategic_flag = true (route to Founder, skip SDR)
pub fn compute_score(account: &Value, trigger_count: i64, has_recent_job_post: bool) -> f64 {
    let mut score: f64 = 50.0;

    let icp = account["icp_segment"].as_str().unwrap_or("");
    if matches!(icp, "saas_50_500" | "saas_500_5000" | "b2b_services") {
        score += 20.0;
    }

    if has_recent_job_post {
        score += 15.0;
    }

    if trigger_count > 1 {
        score += 10.0;
    }

    if account["strategic_flag"].as_bool().unwrap_or(false) {
        score -= 20.0;
    }

    score.clamp(0.0, 100.0)
}

/// Score one account and write `account.scored` evidence.
pub async fn score_account(
    client: &StoreClient,
    account_id: Uuid,
    correlation_id: Uuid,
) -> Result<f64, StoreError> {
    // Fetch account row.
    let acct_resp = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=*&limit=1",
            client.rest("account"),
            account_id
        ))
        .header("Accept", "application/json")
        .send()
        .await?;

    let status = acct_resp.status().as_u16();
    if status >= 300 {
        let text = acct_resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }
    let rows: Vec<Value> = acct_resp.json().await?;
    let account = rows.into_iter().next().unwrap_or_default();

    // Count trigger events for this account.
    let trig_resp = client
        .http
        .get(format!(
            "{}?account_id=eq.{}&select=id",
            client.rest("trigger_event"),
            account_id
        ))
        .header("Accept", "application/json")
        .header("Prefer", "count=exact")
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
    // Content-Range: 0-N/total
    let total: i64 = trig_resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split('/').nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    // Check for recent job_post (last 30 days).
    let job_resp = client
        .http
        .get(format!(
            "{}?account_id=eq.{}&signal_type=eq.job_post&occurred_at=gte.{}&select=id&limit=1",
            client.rest("trigger_event"),
            account_id,
            chrono::Utc::now()
                .checked_sub_signed(chrono::Duration::days(30))
                .map(|d| d.to_rfc3339())
                .unwrap_or_default()
        ))
        .header("Accept", "application/json")
        .send()
        .await?;

    let job_rows: Vec<Value> = job_resp.json().await.unwrap_or_default();
    let has_recent_job_post = !job_rows.is_empty();

    let score = compute_score(&account, total, has_recent_job_post);

    // Persist score back to account.
    let patch_resp = client
        .http
        .patch(format!("{}?id=eq.{}", client.rest("account"), account_id))
        .json(&json!({ "score": score, "updated_at": chrono::Utc::now() }))
        .send()
        .await?;

    let patch_status = patch_resp.status().as_u16();
    if patch_status >= 300 {
        let text = patch_resp.text().await.unwrap_or_default();
        return Err(StoreError::Supabase {
            status: patch_status,
            body: text,
        });
    }

    // Evidence: account.scored
    insert_ledger_row(
        client,
        EvidenceKind::ACCOUNT_SCORED,
        json!({
            "account_id": account_id,
            "score": score,
            "trigger_count": total,
            "has_recent_job_post": has_recent_job_post,
        }),
        correlation_id,
        None,
    )
    .await?;

    Ok(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn acct(icp: &str, strategic: bool) -> Value {
        json!({ "icp_segment": icp, "strategic_flag": strategic })
    }

    #[test]
    fn base_only() {
        // Unknown ICP, no triggers, no job_post, not strategic → 50
        assert_eq!(compute_score(&acct("other", false), 0, false), 50.0);
    }

    #[test]
    fn known_icp_adds_twenty() {
        assert_eq!(compute_score(&acct("saas_50_500", false), 0, false), 70.0);
        assert_eq!(compute_score(&acct("saas_500_5000", false), 0, false), 70.0);
        assert_eq!(compute_score(&acct("b2b_services", false), 0, false), 70.0);
    }

    #[test]
    fn recent_job_post_adds_fifteen() {
        assert_eq!(compute_score(&acct("other", false), 0, true), 65.0);
    }

    #[test]
    fn repeat_trigger_adds_ten() {
        assert_eq!(compute_score(&acct("other", false), 2, false), 60.0);
        assert_eq!(compute_score(&acct("other", false), 1, false), 50.0);
    }

    #[test]
    fn strategic_flag_subtracts_twenty() {
        assert_eq!(compute_score(&acct("other", true), 0, false), 30.0);
    }

    #[test]
    fn all_positive_signals() {
        // 50 + 20 + 15 + 10 = 95
        assert_eq!(compute_score(&acct("saas_50_500", false), 3, true), 95.0);
    }

    #[test]
    fn clamped_between_zero_and_hundred() {
        // 50 - 20 = 30, unknown ICP. Further subtraction impossible but ensure clamp.
        let low = compute_score(&acct("other", true), 0, false);
        assert!((0.0..=100.0).contains(&low));
        // High path should not exceed 100.
        let high = compute_score(&acct("saas_50_500", false), 9, true);
        assert!(high <= 100.0);
    }

    #[test]
    fn missing_fields_default_safely() {
        // Empty object → no icp match, no strategic_flag → base 50
        assert_eq!(compute_score(&json!({}), 0, false), 50.0);
    }
}
