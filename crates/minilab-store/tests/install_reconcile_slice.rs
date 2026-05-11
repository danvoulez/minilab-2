//! Integration tests for the constitutional `install.reconcile` vertical.

use minilab_core::evidence::EvidenceKind;
use minilab_core::SimMode;
use minilab_store::{
    submit_install_reconcile, InstallReconcileInput, InstallReconcileOutcome, StoreClient,
};
use serde_json::{json, Value};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn seed(server: &MockServer, prior_ledger_rows: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prior_ledger_rows))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .mount(server)
        .await;
}

fn client_for(server: &MockServer) -> StoreClient {
    StoreClient::with_mode(server.uri(), "service-key", SimMode::Simulation)
}

fn base_input(correlation_id: Uuid) -> InstallReconcileInput {
    InstallReconcileInput {
        installation_id: Uuid::new_v4(),
        host_id: Uuid::new_v4(),
        desired_manifest: json!({
            "payload_services": [
                {"id": "api", "version": "1"},
                {"id": "worker", "version": "1"}
            ]
        }),
        applied_manifest: None,
        correlation_id,
    }
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
async fn happy_path_writes_plan_steps_and_reconciled() {
    let server = MockServer::start().await;
    seed(&server, vec![]).await;
    let correlation_id = Uuid::new_v4();
    let input = base_input(correlation_id);

    let outcome = submit_install_reconcile(&client_for(&server), input)
        .await
        .expect("install.reconcile must not error on happy path");

    match outcome {
        InstallReconcileOutcome::Reconciled {
            applied_steps,
            skipped_steps,
            ..
        } => {
            assert_eq!(applied_steps, 2);
            assert_eq!(skipped_steps, 0);
        }
        other => panic!("expected reconciled, got {other:?}"),
    }

    let rows = evidence_posts(&server).await;
    assert_eq!(
        kinds_in_order(&rows),
        vec![
            EvidenceKind::INSTALL_RECONCILE_PLANNED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_STEP_APPLIED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_STEP_APPLIED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_RECONCILED.to_string(),
        ]
    );
    assert!(rows
        .iter()
        .all(|row| row["correlation_id"].as_str() == Some(correlation_id.to_string().as_str())));
    assert_eq!(rows[0]["payload"]["step_count"], 2);
}

#[tokio::test]
async fn sub_step_failure_closes_with_failed_after_prior_steps() {
    let server = MockServer::start().await;
    seed(&server, vec![]).await;
    let correlation_id = Uuid::new_v4();
    let mut input = base_input(correlation_id);
    input.desired_manifest = json!({
        "payload_services": [
            {"id": "api", "version": "1"},
            {"id": "worker", "version": "1", "simulate_fail": true}
        ]
    });

    let outcome = submit_install_reconcile(&client_for(&server), input)
        .await
        .expect("simulated step failure is a closed slice outcome");

    match outcome {
        InstallReconcileOutcome::Failed {
            reason_code,
            phase,
            applied_steps,
            ..
        } => {
            assert_eq!(reason_code, "step_failed");
            assert_eq!(phase, "execution");
            assert_eq!(applied_steps, 1);
        }
        other => panic!("expected failed, got {other:?}"),
    }

    let rows = evidence_posts(&server).await;
    assert_eq!(
        kinds_in_order(&rows),
        vec![
            EvidenceKind::INSTALL_RECONCILE_PLANNED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_STEP_APPLIED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_FAILED.to_string(),
        ]
    );
    assert_eq!(rows[2]["payload"]["phase"], "execution");
}

#[tokio::test]
async fn idempotent_rerun_returns_reconciled_without_new_rows() {
    let server = MockServer::start().await;
    let correlation_id = Uuid::new_v4();
    let input = base_input(correlation_id);
    let desired_hash = {
        let first_server = MockServer::start().await;
        seed(&first_server, vec![]).await;
        match submit_install_reconcile(&client_for(&first_server), input.clone())
            .await
            .unwrap()
        {
            InstallReconcileOutcome::Reconciled { desired_hash, .. } => desired_hash,
            other => panic!("expected reconciled, got {other:?}"),
        }
    };

    seed(
        &server,
        vec![json!({
            "kind": EvidenceKind::INSTALL_RECONCILE_RECONCILED,
            "payload": {
                "installation_id": input.installation_id.to_string(),
                "desired_hash": desired_hash,
            }
        })],
    )
    .await;

    let outcome = submit_install_reconcile(&client_for(&server), input)
        .await
        .expect("idempotent rerun should be a semantic no-op");

    assert!(matches!(
        outcome,
        InstallReconcileOutcome::Reconciled {
            applied_steps: 0,
            skipped_steps: 0,
            ..
        }
    ));
    assert!(evidence_posts(&server).await.is_empty());
}

#[tokio::test]
async fn partial_convergence_applies_only_missing_or_changed_services() {
    let server = MockServer::start().await;
    seed(&server, vec![]).await;
    let correlation_id = Uuid::new_v4();
    let mut input = base_input(correlation_id);
    input.applied_manifest = Some(json!({
        "payload_services": [
            {"id": "api", "version": "1"}
        ]
    }));

    let outcome = submit_install_reconcile(&client_for(&server), input)
        .await
        .expect("partial convergence should still close successfully");

    match outcome {
        InstallReconcileOutcome::Reconciled {
            applied_steps,
            skipped_steps,
            ..
        } => {
            assert_eq!(applied_steps, 1);
            assert_eq!(skipped_steps, 1);
        }
        other => panic!("expected reconciled, got {other:?}"),
    }

    let rows = evidence_posts(&server).await;
    assert_eq!(
        kinds_in_order(&rows),
        vec![
            EvidenceKind::INSTALL_RECONCILE_PLANNED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_STEP_APPLIED.to_string(),
            EvidenceKind::INSTALL_RECONCILE_RECONCILED.to_string(),
        ]
    );
    assert_eq!(rows[1]["payload"]["step_id"], "worker");
}
