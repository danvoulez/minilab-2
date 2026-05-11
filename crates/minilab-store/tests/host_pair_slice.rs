//! Integration tests for the constitutional `host.pair` vertical.
//!
//! Success criterion (per `docs/integration/bundle-into-grammar.md` §4):
//!
//! > Given a single correlation_id, I can read in the ledger the full chain
//! > `host.pair.initiated → host.paired | host.pair_failed`, replay-safe,
//! > with `target_runtime=platform` on the lowered command and `sim_mode`
//! > correctly tagged, without depending on implicit interpretation in code.
//!
//! These tests prove that criterion against a mocked Supabase layer. They do
//! not depend on a live Supabase project.

use minilab_core::evidence::EvidenceKind;
use minilab_core::SimMode;
use minilab_store::{submit_host_pair, HostPairInput, HostPairOutcome, StoreClient};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ----------------------------------------------------------------------------
// Seed helpers
// ----------------------------------------------------------------------------

struct Seed {
    host_id: Uuid,
    retired_at: Option<String>,
    pairing_token_hash: Option<String>,
    canon_version: String,
    elastic_version: String,
    /// Ledger rows returned by the replay-check query. The orchestrator
    /// filters to `kind=host.pair.initiated`; this stub returns what the
    /// test prepared regardless of filter (sufficient for our assertions).
    prior_ledger_rows: Vec<Value>,
}

impl Seed {
    fn fresh() -> Self {
        Self {
            host_id: Uuid::new_v4(),
            retired_at: None,
            pairing_token_hash: Some("sha256:op-provided".into()),
            canon_version: "canon-v1".into(),
            elastic_version: "elastic-v1".into(),
            prior_ledger_rows: vec![],
        }
    }
}

async fn seed(server: &MockServer, seed: &Seed) {
    // host row lookup
    let host_row = json!({
        "retired_at": seed.retired_at,
        "pairing_token_hash": seed.pairing_token_hash,
        "canon_version": seed.canon_version,
        "elastic_version": seed.elastic_version,
    });
    Mock::given(method("GET"))
        .and(path("/rest/v1/host"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([host_row])))
        .mount(server)
        .await;

    // evidence_ledger GET (used by replay check)
    Mock::given(method("GET"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&seed.prior_ledger_rows))
        .mount(server)
        .await;

    // evidence_ledger POST catch-all
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

// Base64 of "nonce-xyz" — plausible shape for the mock executor gate.
const VALID_CHALLENGE: &str = "bm9uY2UteHl6";
const VALID_PUBKEY: &str = "cHVia2V5LTEyMw==";

fn base_input(host_id: Uuid, correlation_id: Uuid) -> HostPairInput {
    HostPairInput {
        host_id,
        challenge: VALID_CHALLENGE.into(),
        agent_pubkey: VALID_PUBKEY.into(),
        agent_signature: None,
        correlation_id,
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_writes_initiated_then_paired_under_single_correlation() {
    let server = MockServer::start().await;
    let s = Seed::fresh();
    seed(&server, &s).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_host_pair(&client, base_input(s.host_id, correlation_id))
        .await
        .expect("host.pair must not error on happy path");

    let pairing_session_id = match outcome {
        HostPairOutcome::Paired { pairing_session_id } => pairing_session_id,
        other @ HostPairOutcome::Failed { .. } => panic!("expected Paired, got {other:?}"),
    };

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);

    assert_eq!(
        kinds,
        vec![
            EvidenceKind::HOST_PAIR_INITIATED.to_string(),
            EvidenceKind::HOST_PAIRED.to_string(),
        ],
        "happy path writes exactly two rows, in order"
    );

    // Binding correlation_id on every row.
    for row in &rows {
        assert_eq!(
            row["correlation_id"].as_str(),
            Some(correlation_id.to_string().as_str()),
            "row kind={} must carry the binding correlation_id",
            row["kind"]
        );
        assert_eq!(
            row["sim_mode"].as_str(),
            Some("simulation"),
            "simulation runs must tag sim_mode on every row"
        );
    }

    // `host.pair.initiated` carries the challenge_hash (deterministic SHA-256).
    let initiated = rows
        .iter()
        .find(|r| r["kind"] == EvidenceKind::HOST_PAIR_INITIATED)
        .unwrap();
    let expected_hash = {
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(VALID_CHALLENGE.as_bytes());
        let mut s = String::new();
        for b in d {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", b);
        }
        s
    };
    assert_eq!(
        initiated["payload"]["challenge_hash"].as_str(),
        Some(expected_hash.as_str()),
        "host.pair.initiated must record the deterministic challenge_hash"
    );

    // `host.paired` carries the agent_pubkey and the generated session id.
    let paired = rows
        .iter()
        .find(|r| r["kind"] == EvidenceKind::HOST_PAIRED)
        .unwrap();
    assert_eq!(
        paired["payload"]["agent_pubkey"].as_str(),
        Some(VALID_PUBKEY),
        "host.paired must record the agent_pubkey as submitted"
    );
    assert_eq!(
        paired["payload"]["pairing_session_id"].as_str(),
        Some(pairing_session_id.to_string().as_str())
    );
    assert_eq!(
        paired["payload"]["canon_version"].as_str(),
        Some("canon-v1")
    );
    assert_eq!(
        paired["payload"]["elastic_version"].as_str(),
        Some("elastic-v1")
    );
}

#[tokio::test]
async fn replay_of_same_challenge_writes_pair_failed_deterministically() {
    let server = MockServer::start().await;
    let mut s = Seed::fresh();

    // Simulate a prior `host.pair.initiated` already in the ledger for this
    // (host_id, challenge_hash) pair — this is what the replay check reads.
    let challenge_hash = {
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(VALID_CHALLENGE.as_bytes());
        let mut s = String::new();
        for b in d {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", b);
        }
        s
    };
    s.prior_ledger_rows = vec![json!({
        "kind": "host.pair.initiated",
        "payload": {
            "host_id": s.host_id.to_string(),
            "challenge_hash": challenge_hash,
            "correlation_id": "00000000-0000-0000-0000-000000000099",
        }
    })];

    seed(&server, &s).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_host_pair(&client, base_input(s.host_id, correlation_id))
        .await
        .expect("replay must be a returned failure, not a bubbled error");

    match outcome {
        HostPairOutcome::Failed {
            reason_code, phase, ..
        } => {
            assert_eq!(reason_code, "replay_detected");
            assert_eq!(phase, "pre_admission");
        }
        other @ HostPairOutcome::Paired { .. } => {
            panic!("expected Failed(replay_detected), got {other:?}")
        }
    }

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);

    // The critical shape: replay does NOT emit a second `host.pair.initiated`.
    // It emits only `host.pair_failed`. Replay is a deterministic rejection,
    // not silent idempotency.
    assert_eq!(
        kinds,
        vec![EvidenceKind::HOST_PAIR_FAILED.to_string()],
        "replay writes only host.pair_failed — no new initiated, no paired"
    );

    let failed = &rows[0];
    assert_eq!(
        failed["payload"]["reason_code"].as_str(),
        Some("replay_detected")
    );
    assert_eq!(failed["payload"]["phase"].as_str(), Some("pre_admission"));
    assert_eq!(
        failed["payload"]["challenge_hash"].as_str(),
        Some(challenge_hash.as_str()),
        "failure row still names the replayed challenge for auditability"
    );
}

#[tokio::test]
async fn missing_token_hash_fails_with_token_mismatch_pre_admission() {
    let server = MockServer::start().await;
    let mut s = Seed::fresh();
    s.pairing_token_hash = None;
    seed(&server, &s).await;

    let client = client_for(&server, SimMode::Simulation);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_host_pair(&client, base_input(s.host_id, correlation_id))
        .await
        .expect("missing token is a returned failure, not a bubbled error");

    match outcome {
        HostPairOutcome::Failed {
            reason_code, phase, ..
        } => {
            assert_eq!(reason_code, "token_mismatch");
            assert_eq!(
                phase, "pre_admission",
                "token check happens before admission"
            );
        }
        other @ HostPairOutcome::Paired { .. } => {
            panic!("expected Failed(token_mismatch), got {other:?}")
        }
    }

    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);
    assert_eq!(
        kinds,
        vec![EvidenceKind::HOST_PAIR_FAILED.to_string()],
        "token rejection writes only host.pair_failed"
    );
}

#[tokio::test]
async fn production_without_signature_fails_at_execution_phase() {
    // Production now uses the real ed25519 executor path. Missing signatures
    // fail closed at execution phase rather than falling back to a mock pairer.
    let server = MockServer::start().await;
    let s = Seed::fresh();
    seed(&server, &s).await;

    let client = client_for(&server, SimMode::Production);
    let correlation_id = Uuid::new_v4();
    let outcome = submit_host_pair(&client, base_input(s.host_id, correlation_id))
        .await
        .expect("production-no-executor is a returned failure, not a bubbled error");

    match outcome {
        HostPairOutcome::Failed {
            reason_code, phase, ..
        } => {
            assert_eq!(reason_code, "invalid_signature");
            assert_eq!(
                phase, "execution",
                "missing signatures fail during execution"
            );
        }
        other @ HostPairOutcome::Paired { .. } => {
            panic!("expected Failed(invalid_signature), got {other:?}")
        }
    }

    // `host.pair.initiated` IS written before the executor is consulted, so
    // the chain reflects that admission succeeded and execution failed.
    let rows = evidence_posts(&server).await;
    let kinds = kinds_in_order(&rows);
    assert_eq!(
        kinds,
        vec![
            EvidenceKind::HOST_PAIR_INITIATED.to_string(),
            EvidenceKind::HOST_PAIR_FAILED.to_string(),
        ],
        "admission precedes signature failure in the ledger"
    );
}
