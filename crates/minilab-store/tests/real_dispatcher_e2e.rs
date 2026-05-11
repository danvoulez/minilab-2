//! End-to-end tests for PR 5a: **operational program → compiled plan →
//! real dispatcher → live slice orchestrator → ExecutionReport**.
//!
//! Success criteria (from the PR 5a review):
//!
//! 1. A program whose only verb is `host.pair` drives the real
//!    `submit_host_pair` orchestrator through the real dispatcher, and the
//!    report carries a concrete `evidence_ref` (the pairing session id).
//! 2. A program whose only verb is `outbound.send` at least reaches the
//!    real orchestrator's admissibility chain and produces a constitutional
//!    outcome (Success / Failure) — no silent passes, no panics.
//! 3. A program whose verb has no live slice bound produces a **Blocked**
//!    outcome and halting is respected.
//!
//! The tests reuse the wiremock seeding pattern established in
//! `host_pair_slice.rs` / `outbound_send_slice.rs`; they do not depend on a
//! live Supabase project.

use constitutional_runtime::{
    execute_compiled_plan_async, parse_program, plan_operational_program, AdmissibilityContext,
    CapabilityManifest, MinilabRuntimeLowerer, NodeOutcome, PolicyClass, PrimitiveName,
};
use minilab_core::SimMode;
use minilab_store::real_dispatcher::RealDispatcher;
use minilab_store::StoreClient;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn manifest_full() -> CapabilityManifest {
    CapabilityManifest {
        substrate_id: "local".into(),
        substrate_version: "1".into(),
        supported_primitives: BTreeSet::from_iter([
            PrimitiveName::Observe,
            PrimitiveName::Collect,
            PrimitiveName::Fetch,
            PrimitiveName::Compress,
            PrimitiveName::Classify,
            PrimitiveName::Prioritize,
            PrimitiveName::Compare,
            PrimitiveName::Route,
            PrimitiveName::Schedule,
            PrimitiveName::Execute,
            PrimitiveName::Emit,
            PrimitiveName::Persist,
            PrimitiveName::Confirm,
            PrimitiveName::Cancel,
            PrimitiveName::Reconcile,
        ]),
        declared_guarantees: BTreeSet::from(["evidence.write".into()]),
        ..Default::default()
    }
}

fn ctx() -> AdmissibilityContext {
    AdmissibilityContext {
        policy_class: PolicyClass::C,
        runtime_permitted: true,
        at_execution_boundary: true,
        require_evidence_closure: true,
    }
}

fn client_for(server: &MockServer, mode: SimMode) -> StoreClient {
    StoreClient::with_mode(server.uri(), "service-key", mode)
}

// Base64 shapes expected by the host.pair mock executor gate.
const VALID_CHALLENGE: &str = "bm9uY2UteHl6";
const VALID_PUBKEY: &str = "cHVia2V5LTEyMw==";

// ---------------------------------------------------------------------------
// Caso 1: host.pair through the real dispatcher
// ---------------------------------------------------------------------------

async fn seed_host_pair_happy(server: &MockServer) {
    let host_row = json!({
        "retired_at": Value::Null,
        "pairing_token_hash": "sha256:op-provided",
        "canon_version": "canon-v1",
        "elastic_version": "elastic-v1",
    });
    Mock::given(method("GET"))
        .and(path("/rest/v1/host"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([host_row])))
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

#[tokio::test]
async fn host_pair_program_drives_live_slice_and_report_carries_evidence_ref() {
    let server = MockServer::start().await;
    seed_host_pair_happy(&server).await;

    let host_id = Uuid::new_v4();
    let correlation_id = Uuid::new_v4();
    let program_text = format!(
        "host.pair host_id={host} challenge={ch} agent_pubkey={pk} correlation_id={cid}\n",
        host = host_id,
        ch = VALID_CHALLENGE,
        pk = VALID_PUBKEY,
        cid = correlation_id,
    );
    let program = parse_program(&program_text).expect("parse");

    let plan =
        plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
            .expect("plan");
    assert_eq!(plan.node_plans.len(), 1);
    assert_eq!(plan.node_plans[0].command.namespace, "host");
    assert_eq!(plan.node_plans[0].command.verb, "pair");

    let dispatcher = RealDispatcher::new(client_for(&server, SimMode::Simulation));
    let report = execute_compiled_plan_async(&plan, &dispatcher).await;

    assert!(
        report.fully_succeeded(),
        "report should fully succeed, got halted_on={:?}, results={:?}",
        report.halted_on,
        report.results
    );
    match &report.results[0].outcome {
        NodeOutcome::Success {
            evidence_ref,
            detail,
        } => {
            let ev = evidence_ref
                .as_ref()
                .expect("real dispatcher must surface the pairing_session_id as evidence_ref");
            // Must parse as a UUID — the slice returned a real session id.
            Uuid::parse_str(ev).expect("evidence_ref must be a UUID");
            let detail = detail.as_ref().expect("detail must be present");
            assert_eq!(detail["slice"], "host.pair");
        }
        other => panic!("expected Success, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Caso 2: outbound.send reaches the live orchestrator
// ---------------------------------------------------------------------------
//
// The outbound chain has many upstream checks (contact → account, eligibility,
// premium, opt-out, policy). Exercising every branch end-to-end would duplicate
// `tests/outbound_send_slice.rs`. For PR 5a we only need to prove that the
// trilho actually drives the live chain far enough to produce a constitutional
// outcome — we seed the minimum so it reaches the `contact_suppressed` branch,
// which is a closed `Rejected` outcome.

async fn seed_outbound_contact_missing(server: &MockServer) {
    // Contact lookup returns an empty array → orchestrator takes the
    // `contact not found or suppressed` branch and writes `outbound.failed`.
    Mock::given(method("GET"))
        .and(path("/rest/v1/contact"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/evidence_ledger"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn outbound_send_program_reaches_live_orchestrator_and_emits_closed_outcome() {
    let server = MockServer::start().await;
    seed_outbound_contact_missing(&server).await;

    let contact_id = Uuid::new_v4();
    let campaign_id = Uuid::new_v4();
    let correlation_id = Uuid::new_v4();
    let program_text = format!(
        "outbound.send contact_id={c} campaign_id={ca} channel=email correlation_id={cor}\n",
        c = contact_id,
        ca = campaign_id,
        cor = correlation_id,
    );
    let program = parse_program(&program_text).expect("parse");
    let plan =
        plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
            .expect("plan");
    assert_eq!(plan.node_plans[0].command.namespace, "outbound");
    assert_eq!(plan.node_plans[0].command.verb, "send");

    let dispatcher = RealDispatcher::new(client_for(&server, SimMode::Simulation));
    let report = execute_compiled_plan_async(&plan, &dispatcher).await;

    // The chain refused admissibility at the contact resolution step. The
    // report must mark this as a Failure with the closed reason code the
    // orchestrator uses — not a Success, not a Blocked.
    match &report.results[0].outcome {
        NodeOutcome::Failure {
            reason_code,
            detail,
            runtime_failure,
        } => {
            assert_eq!(reason_code, "contact_suppressed");
            assert!(detail.is_some());
            // PR 5c: the real dispatcher must attach a structured failure
            // tagged at the Execution stage, with the same reason_code.
            let rf = runtime_failure
                .as_ref()
                .expect("real dispatcher must emit RuntimeFailure on Failure");
            assert_eq!(rf.stage().as_str(), "execution");
            assert_eq!(rf.reason_code(), "contact_suppressed");
        }
        other => panic!("expected Failure from live orchestrator, got {other:?}"),
    }
    assert_eq!(report.halted_on.as_ref().map(|n| n.0.as_str()), Some("n0"));
}

// ---------------------------------------------------------------------------
// Caso 3: unsupported verb → Blocked + halting respected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unsupported_verb_blocks_and_halts_remainder_of_program() {
    // The program mixes a supported live verb with a pseudo-verb that has
    // no live slice binding. The unsupported one must be Blocked, halting
    // the run; any subsequent node is Skipped without reaching the
    // dispatcher.
    let server = MockServer::start().await;
    // Seed just enough so the host.pair branch *could* succeed if reached.
    seed_host_pair_happy(&server).await;

    // The first line is a verb that parses, maps to an Execute IR, but has
    // no live slice binding in `minilab-store::dispatcher`. We piggy-back on
    // `cmd.cancel` — it maps to IRPrimitive::Cancel which the
    // MinilabRuntimeLowerer lowers to `cmd.cancel` as an operational
    // command. The real dispatcher does not match that triple and returns
    // the honest Contract error → Blocked.
    let host_id = Uuid::new_v4();
    let correlation_id = Uuid::new_v4();
    let program_text = format!(
        "cmd.cancel id=abc-123\nhost.pair host_id={host} challenge={ch} agent_pubkey={pk} correlation_id={cid}\n",
        host = host_id,
        ch = VALID_CHALLENGE,
        pk = VALID_PUBKEY,
        cid = correlation_id,
    );
    let program = parse_program(&program_text).expect("parse");

    let plan =
        plan_operational_program(&program, &[manifest_full()], &ctx(), &MinilabRuntimeLowerer)
            .expect("plan");
    assert_eq!(plan.node_plans.len(), 2);

    let dispatcher = RealDispatcher::new(client_for(&server, SimMode::Simulation));
    let report = execute_compiled_plan_async(&plan, &dispatcher).await;

    assert_eq!(report.halted_on.as_ref().map(|n| n.0.as_str()), Some("n0"));
    match &report.results[0].outcome {
        NodeOutcome::Blocked { reason } => {
            assert!(
                reason.contains("unsupported operational command"),
                "reason={reason}"
            );
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    // Second node was never dispatched.
    assert!(!report.results[1].dispatched);
    match &report.results[1].outcome {
        NodeOutcome::Skipped { after } => assert_eq!(after.0, "n0"),
        other => panic!("expected Skipped, got {other:?}"),
    }
}
