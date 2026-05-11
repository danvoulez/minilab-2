//! Integration tests for the canonical runtime dispatch handoff:
//!
//! ```text
//!   IR Execute node
//!     -> lower_execute_action
//!     -> OperationalCommand
//!     -> dispatch_operational_command
//!     -> slice orchestrator
//! ```
//!
//! The slice-specific tests already prove each orchestrator in isolation.
//! These tests prove the last formal gap: the runtime can now consume the
//! canonical lowered command and hand it off to the right act chain.

use std::collections::BTreeMap;

use constitutional_runtime::RuntimeTarget;
use minilab_core::evidence::EvidenceKind;
use minilab_core::SimMode;
use minilab_store::{
    dispatch_operational_command, lower_execute_action, DispatchOutcome, StoreClient,
};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct OutboundSeed {
    contact_id: Uuid,
    account_id: Uuid,
    campaign_id: Uuid,
    message_family_id: Uuid,
}

impl OutboundSeed {
    fn fresh() -> Self {
        Self {
            contact_id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            campaign_id: Uuid::new_v4(),
            message_family_id: Uuid::new_v4(),
        }
    }
}

struct HostSeed {
    host_id: Uuid,
}

impl HostSeed {
    fn fresh() -> Self {
        Self {
            host_id: Uuid::new_v4(),
        }
    }
}

async fn seed_outbound_reads(server: &MockServer, seed: &OutboundSeed) {
    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "account_id"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{ "account_id": seed.account_id }])),
        )
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "opt_out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "opt_out": false }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .and(query_param("select", "id,email,phone_e164,opt_out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.contact_id,
            "email": "buyer@acme.example",
            "phone_e164": null,
            "opt_out": false,
        }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/message_family"))
        .and(query_param("select", "claims_allowed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "claims_allowed": [] }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/message_family"))
        .and(query_param("select", "id,template_ref"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.message_family_id,
            "template_ref": "templates/pain_v1.md",
        }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/policy_rule"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/account"))
        .and(query_param("select", "icp_segment"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{ "icp_segment": "saas_50_500" }])),
        )
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/account"))
        .and(query_param("select", "id,name,domain"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.account_id,
            "name": "Acme",
            "domain": "acme.example",
        }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/campaign"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": seed.campaign_id,
            "name": "Outbound wedge",
            "channel": "email",
        }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .mount(server)
        .await;
}

async fn seed_host_reads(server: &MockServer, seed: &HostSeed) {
    Mock::given(method("GET"))
        .and(path("/rest/v1/host"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "retired_at": null,
            "pairing_token_hash": "sha256:op-provided",
            "canon_version": "canon-v1",
            "elastic_version": "elastic-v1",
        }])))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .mount(server)
        .await;

    let _ = seed;
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

#[tokio::test]
async fn lowered_outbound_send_command_dispatches_into_the_business_slice() {
    let server = MockServer::start().await;
    let seed = OutboundSeed::fresh();
    seed_outbound_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let mut params = BTreeMap::new();
    params.insert("contact_id".into(), json!(seed.contact_id));
    params.insert("campaign_id".into(), json!(seed.campaign_id));
    params.insert("channel".into(), json!("email"));
    params.insert("correlation_id".into(), json!(correlation_id));

    let (_, command) = lower_execute_action("dispatch-outbound", "outbound.send", params)
        .expect("outbound.send must lower");
    assert_eq!(command.target_runtime, RuntimeTarget::Provider);

    let outcome = dispatch_operational_command(&client, command)
        .await
        .expect("dispatcher must route outbound.send");

    assert!(
        matches!(
            outcome,
            DispatchOutcome::OutboundSend(minilab_store::OutboundSendOutcome::Sent { .. })
        ),
        "dispatcher must land in the outbound.send slice"
    );

    let rows = evidence_posts(&server).await;
    assert_eq!(
        kinds_in_order(&rows),
        vec![
            EvidenceKind::ELIGIBILITY_EVALUATED.to_string(),
            EvidenceKind::PREMIUM_EVALUATED.to_string(),
            EvidenceKind::POLICY_EVALUATED.to_string(),
            EvidenceKind::OUTBOUND_ADMITTED.to_string(),
            EvidenceKind::OUTBOUND_SENT.to_string(),
        ]
    );
}

#[tokio::test]
async fn lowered_host_pair_command_dispatches_into_the_physical_slice() {
    let server = MockServer::start().await;
    let seed = HostSeed::fresh();
    seed_host_reads(&server, &seed).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let mut params = BTreeMap::new();
    params.insert("host_id".into(), json!(seed.host_id));
    params.insert("challenge".into(), json!("bm9uY2UteHl6"));
    params.insert("agent_pubkey".into(), json!("cHVia2V5LTEyMw=="));
    params.insert("correlation_id".into(), json!(correlation_id));

    let (_, command) = lower_execute_action("dispatch-host-pair", "host.pair", params)
        .expect("host.pair must lower");
    assert_eq!(command.target_runtime, RuntimeTarget::Platform);

    let outcome = dispatch_operational_command(&client, command)
        .await
        .expect("dispatcher must route host.pair");

    assert!(
        matches!(
            outcome,
            DispatchOutcome::HostPair(minilab_store::HostPairOutcome::Paired { .. })
        ),
        "dispatcher must land in the host.pair slice"
    );

    let rows = evidence_posts(&server).await;
    assert_eq!(
        kinds_in_order(&rows),
        vec![
            EvidenceKind::HOST_PAIR_INITIATED.to_string(),
            EvidenceKind::HOST_PAIRED.to_string(),
        ]
    );
}
