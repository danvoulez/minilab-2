//! Integration tests for the constitutional outbound.send vertical.
//!
//! Success criterion (per `docs/integration/huge-into-grammar.md` §4):
//!
//! > Given a single correlation_id, we can read in the ledger a complete
//! > constitutional chain of admissibility → admission → send/failure → reply
//! > follow-on, without depending on implicit interpretation in code.
//!
//! These tests prove that criterion against a mocked Supabase layer. They do
//! not depend on a live Supabase project and run in CI.

use minilab_core::evidence::EvidenceKind;
use minilab_core::SimMode;
use minilab_store::{submit_outbound_send, OutboundSendInput, OutboundSendOutcome, StoreClient};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ----------------------------------------------------------------------------
// Seed helpers
// ----------------------------------------------------------------------------

struct Seed {
    contact_id: Uuid,
    account_id: Uuid,
    campaign_id: Uuid,
    message_family_id: Uuid,
    channel: String,
    contact_opt_out: bool,
    existing_optout_rows: Vec<Value>,
    policy_rules: Vec<Value>,
}

impl Seed {
    fn fresh() -> Self {
        Self {
            contact_id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            campaign_id: Uuid::new_v4(),
            message_family_id: Uuid::new_v4(),
            channel: "email".into(),
            contact_opt_out: false,
            existing_optout_rows: vec![],
            policy_rules: vec![],
        }
    }
}

/// Seed read-side mocks against a fresh `MockServer`.
async fn seed_reads(server: &MockServer, seed: &Seed) {
    // contact (select=account_id) — resolve_account_for_contact
    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "account_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "account_id": seed.account_id.to_string(),
        }])))
        .mount(server)
        .await;

    // contact (select=opt_out) — optout_gate legacy column
    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "opt_out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "opt_out": seed.contact_opt_out,
        }])))
        .mount(server)
        .await;

    // contact (full row, select=id,email,phone_e164,opt_out) — dispatch_sequence
    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "id,email,phone_e164,opt_out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.contact_id.to_string(),
            "email": "buyer@acme.example",
            "phone_e164": null,
            "opt_out": seed.contact_opt_out,
        }])))
        .mount(server)
        .await;

    // message_family (select=claims_allowed) — eligibility
    Mock::given(method("GET"))
        .and(path("/rest/v1/message_family"))
        .and(query_param("select", "claims_allowed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "claims_allowed": [],
        }])))
        .mount(server)
        .await;

    // message_family (select=id,template_ref) — dispatch_sequence
    Mock::given(method("GET"))
        .and(path("/rest/v1/message_family"))
        .and(query_param("select", "id,template_ref"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.message_family_id.to_string(),
            "template_ref": "templates/pain_v1.md",
        }])))
        .mount(server)
        .await;

    // policy_rule — evaluate_and_record_send_policy
    Mock::given(method("GET"))
        .and(path("/rest/v1/policy_rule"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&seed.policy_rules))
        .mount(server)
        .await;

    // account (select=icp_segment) — policy
    Mock::given(method("GET"))
        .and(path("/rest/v1/account"))
        .and(query_param("select", "icp_segment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "icp_segment": "saas_50_500",
        }])))
        .mount(server)
        .await;

    // account (select=id,name,domain) — dispatch_sequence
    Mock::given(method("GET"))
        .and(path("/rest/v1/account"))
        .and(query_param("select", "id,name,domain"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.account_id.to_string(),
            "name": "Acme",
            "domain": "acme.example",
        }])))
        .mount(server)
        .await;

    // campaign (select=id,name,channel) — dispatch_sequence
    Mock::given(method("GET"))
        .and(path("/rest/v1/campaign"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.campaign_id.to_string(),
            "name": "Outbound wedge",
            "channel": seed.channel,
        }])))
        .mount(server)
        .await;

    // evidence_ledger GET (kind=eq.optout.asserted) — optout_gate ledger read
    Mock::given(method("GET"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&seed.existing_optout_rows))
        .mount(server)
        .await;

    // evidence_ledger POST — catch all; capture for later assertion.
    Mock::given(method("POST"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .mount(server)
        .await;
}

fn client_for(server: &MockServer, mode: SimMode) -> StoreClient {
    StoreClient::with_mode(server.uri(), "service-key", mode)
}

async fn evidence_posts(server: &MockServer) -> Vec<Value> {
    let received = server.received_requests().await.unwrap_or_default();
    received
        .into_iter()
        .filter(|r| {
            r.method == wiremock::http::Method::POST && r.url.path() == "/rest/v1/evidence_ledger"
        })
        .filter_map(|r| serde_json::from_slice::<Value>(&r.body).ok())
        .collect()
}

fn kinds_in_order(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r["kind"].as_str().map(str::to_owned))
        .collect()
}

fn correlation_ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| r["correlation_id"].as_str().map(str::to_owned))
        .collect()
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_writes_full_constitutional_chain() {
    let server = MockServer::start().await;
    let seed = Seed::fresh();
    seed_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_outbound_send(
        &client,
        OutboundSendInput {
            contact_id: seed.contact_id,
            campaign_id: seed.campaign_id,
            channel: seed.channel.clone(),
            correlation_id,
        },
    )
    .await
    .expect("orchestrator must not error on happy path");

    assert!(
        matches!(outcome, OutboundSendOutcome::Sent { .. }),
        "expected Sent, got {:?}",
        outcome
    );

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);

    assert_eq!(
        kinds,
        vec![
            EvidenceKind::ELIGIBILITY_EVALUATED.to_string(),
            EvidenceKind::PREMIUM_EVALUATED.to_string(),
            EvidenceKind::POLICY_EVALUATED.to_string(),
            EvidenceKind::OUTBOUND_ADMITTED.to_string(),
            EvidenceKind::OUTBOUND_SENT.to_string(),
        ],
        "constitutional chain must be complete and ordered"
    );

    // All rows share the same correlation_id.
    let correlations = correlation_ids(&rows);
    assert!(
        correlations
            .iter()
            .all(|c| c == &correlation_id.to_string()),
        "every evidence row must carry the binding correlation_id (got {:?})",
        correlations
    );

    // sim_mode must be tagged on every row (written by insert_ledger_row).
    for row in &rows {
        assert_eq!(
            row["sim_mode"].as_str(),
            Some("simulation"),
            "simulation sends must tag sim_mode; row kind={}",
            row["kind"]
        );
    }

    // Each evaluator must state its result as "admissible" on the happy path.
    let find = |kind: &str| rows.iter().find(|r| r["kind"] == kind).unwrap();
    assert_eq!(
        find(EvidenceKind::ELIGIBILITY_EVALUATED)["payload"]["result"].as_str(),
        Some("admissible")
    );
    assert_eq!(
        find(EvidenceKind::ELIGIBILITY_EVALUATED)["payload"]["asserts_new_claim"],
        json!(false),
        "eligibility must not be treated as a claim assertion"
    );
    assert_eq!(
        find(EvidenceKind::PREMIUM_EVALUATED)["payload"]["result"].as_str(),
        Some("admissible")
    );
    assert_eq!(
        find(EvidenceKind::POLICY_EVALUATED)["payload"]["result"].as_str(),
        Some("admissible")
    );
}

#[tokio::test]
async fn optout_ledger_row_blocks_before_dispatch() {
    let server = MockServer::start().await;
    let mut seed = Seed::fresh();
    seed.existing_optout_rows = vec![json!({
        "correlation_id": "11111111-1111-1111-1111-111111111111",
        "payload": {
            "contact_id": seed.contact_id.to_string(),
            "channel": seed.channel,
            "correlation_id": "11111111-1111-1111-1111-111111111111",
        }
    })];
    seed_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_outbound_send(
        &client,
        OutboundSendInput {
            contact_id: seed.contact_id,
            campaign_id: seed.campaign_id,
            channel: seed.channel.clone(),
            correlation_id,
        },
    )
    .await
    .expect("orchestrator must not error when blocked");

    match outcome {
        OutboundSendOutcome::Rejected { reason_code, .. } => {
            assert_eq!(reason_code, "optout_active");
        }
        other => panic!("expected Rejected(optout_active), got {other:?}"),
    }

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);

    // Opt-out gate fires AFTER eligibility + premium but BEFORE policy rules.
    // The chain must therefore contain exactly three evaluations plus the
    // failure — no admitted, no sent.
    assert_eq!(
        kinds,
        vec![
            EvidenceKind::ELIGIBILITY_EVALUATED.to_string(),
            EvidenceKind::PREMIUM_EVALUATED.to_string(),
            EvidenceKind::OUTBOUND_FAILED.to_string(),
        ],
        "opt-out must block before policy evaluation and before admission"
    );

    let failed = rows
        .iter()
        .find(|r| r["kind"] == EvidenceKind::OUTBOUND_FAILED)
        .unwrap();
    assert_eq!(
        failed["payload"]["reason_code"].as_str(),
        Some("optout_active")
    );
    assert_eq!(
        failed["payload"]["phase"].as_str(),
        Some("pre_admission"),
        "opt-out rejection must be recorded as pre-admission failure"
    );
    assert_eq!(
        failed["payload"]["correlation_id"].as_str(),
        Some(correlation_id.to_string().as_str())
    );
}

#[tokio::test]
async fn legacy_contact_opt_out_column_also_blocks() {
    let server = MockServer::start().await;
    let mut seed = Seed::fresh();
    seed.contact_opt_out = true;
    seed_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_outbound_send(
        &client,
        OutboundSendInput {
            contact_id: seed.contact_id,
            campaign_id: seed.campaign_id,
            channel: seed.channel.clone(),
            correlation_id,
        },
    )
    .await
    .expect("orchestrator must not error when blocked via legacy column");

    assert!(
        matches!(outcome, OutboundSendOutcome::Rejected { ref reason_code, .. } if reason_code == "optout_active"),
        "legacy opt_out column must also trigger optout_active (transition coexistence), got {outcome:?}"
    );
}

#[tokio::test]
async fn blocking_policy_rule_rejects_with_policy_blocked() {
    let server = MockServer::start().await;
    let mut seed = Seed::fresh();
    seed.policy_rules = vec![json!({
        "name": "maintenance",
        "expression": { "rule_kind": "block_all" }
    })];
    seed_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_outbound_send(
        &client,
        OutboundSendInput {
            contact_id: seed.contact_id,
            campaign_id: seed.campaign_id,
            channel: seed.channel.clone(),
            correlation_id,
        },
    )
    .await
    .expect("orchestrator must not error when policy blocks");

    assert!(
        matches!(outcome, OutboundSendOutcome::Rejected { ref reason_code, .. } if reason_code == "policy_blocked"),
        "block_all rule must yield policy_blocked, got {outcome:?}"
    );

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);
    assert_eq!(
        kinds,
        vec![
            EvidenceKind::ELIGIBILITY_EVALUATED.to_string(),
            EvidenceKind::PREMIUM_EVALUATED.to_string(),
            EvidenceKind::POLICY_EVALUATED.to_string(),
            EvidenceKind::OUTBOUND_FAILED.to_string(),
        ],
        "policy block must write policy.evaluated (blocked) before outbound.failed; no admitted, no sent"
    );

    let policy_row = rows
        .iter()
        .find(|r| r["kind"] == EvidenceKind::POLICY_EVALUATED)
        .unwrap();
    assert_eq!(
        policy_row["payload"]["result"].as_str(),
        Some("blocked"),
        "policy.evaluated must record the blocked result on the negative path"
    );
}
