//! Real-world adapter from the constitutional runtime's plan executor to
//! the live slice orchestrators.
//!
//! PR 5a's goal in one sentence: **plug the trilho (operational program →
//! compiled plan → execution) into the live slices (`outbound.send`,
//! `host.pair`) without rewriting either**.
//!
//! The adapter is intentionally thin:
//!
//! ```text
//! execute_compiled_plan_async
//!     ↓ AsyncDispatcher::dispatch_async
//! RealDispatcher
//!     ↓ dispatch_operational_command       (minilab-store, already landed)
//!     ↓ submit_outbound_send | submit_host_pair
//! ```
//!
//! It does **not**:
//!
//! - reimplement admissibility, capability, evidence, or policy,
//! - add retries or parallelism,
//! - introduce new verbs beyond the two live slices,
//! - integrate [`constitutional_runtime::RuntimeFailure`] or
//!   [`constitutional_runtime::IdempotencyContract`] into the outcome shape
//!   (those are PR 5c territory).
//!
//! The only thing it owns is the **translation** between two closed outcome
//! vocabularies:
//!
//! | live slice outcome              | executor `DispatchOutcome`                 |
//! |---------------------------------|--------------------------------------------|
//! | `OutboundSendOutcome::Sent`     | `Success { evidence_ref: provider_message_id }` |
//! | `OutboundSendOutcome::Rejected` | `Failure { reason_code, detail }`          |
//! | `OutboundSendOutcome::ProviderError` | `Failure { reason_code, detail }`     |
//! | `HostPairOutcome::Paired`       | `Success { evidence_ref: pairing_session_id }`  |
//! | `HostPairOutcome::Failed`       | `Failure { reason_code, detail: "[phase] detail" }` |
//! | dispatcher contract mismatch    | `Blocked { reason }`                       |
//! | other `StoreError`              | `Failure { reason_code: "store_error" }`   |
//!
//! That table is the whole constitutional meaning of this module.

use std::pin::Pin;

use constitutional_runtime::{
    AsyncDispatcher, DispatchOutcome as ExecDispatchOutcome, NodeId, OperationalCommand,
    RuntimeFailure,
};
use core::future::Future;
use serde_json::json;

use crate::client::{StoreClient, StoreError};
use crate::dispatcher::{dispatch_operational_command, DispatchOutcome as StoreDispatchOutcome};
use crate::host_pair::HostPairOutcome;
use crate::install_reconcile::InstallReconcileOutcome;
use crate::outbound_orchestrator::OutboundSendOutcome;

/// Real runtime adapter that routes lowered commands into the landed slices.
///
/// Holds a [`StoreClient`] by value (cheaply cloneable) so callers can
/// share it across tasks. One `RealDispatcher` per execution pass is the
/// intended usage; reuse is safe because the dispatcher itself is stateless.
#[derive(Clone)]
pub struct RealDispatcher {
    client: StoreClient,
}

impl RealDispatcher {
    pub fn new(client: StoreClient) -> Self {
        Self { client }
    }

    /// Run the real dispatch and map the result. Separate from the trait
    /// impl so callers that are already async can bypass the `Box<Future>`
    /// layer when they want to, and so tests can exercise the mapping
    /// directly.
    pub async fn dispatch_real(
        &self,
        node_id: &NodeId,
        command: &OperationalCommand,
    ) -> ExecDispatchOutcome {
        match dispatch_operational_command(&self.client, command.clone()).await {
            Ok(StoreDispatchOutcome::OutboundSend(OutboundSendOutcome::Sent {
                provider,
                provider_message_id,
                channel,
            })) => ExecDispatchOutcome::Success {
                evidence_ref: Some(provider_message_id.clone()),
                detail: Some(json!({
                    "slice": "outbound.send",
                    "provider": provider,
                    "channel": channel,
                    "provider_message_id": provider_message_id,
                })),
            },
            Ok(StoreDispatchOutcome::OutboundSend(OutboundSendOutcome::Rejected {
                reason_code,
                detail,
            })) => {
                let rf = execution_failure(node_id, "outbound.send", &reason_code, &detail);
                ExecDispatchOutcome::Failure {
                    reason_code,
                    detail: Some(detail),
                    runtime_failure: Some(rf),
                }
            }
            Ok(StoreDispatchOutcome::OutboundSend(OutboundSendOutcome::ProviderError {
                reason_code,
                detail,
            })) => {
                let rf = execution_failure(node_id, "outbound.send", &reason_code, &detail);
                ExecDispatchOutcome::Failure {
                    reason_code,
                    detail: Some(detail),
                    runtime_failure: Some(rf),
                }
            }
            Ok(StoreDispatchOutcome::HostPair(HostPairOutcome::Paired { pairing_session_id })) => {
                ExecDispatchOutcome::Success {
                    evidence_ref: Some(pairing_session_id.to_string()),
                    detail: Some(json!({
                        "slice": "host.pair",
                        "pairing_session_id": pairing_session_id,
                    })),
                }
            }
            Ok(StoreDispatchOutcome::HostPair(HostPairOutcome::Failed {
                reason_code,
                reason_detail,
                phase,
            })) => {
                let composed_detail = format!("[{phase}] {reason_detail}");
                let rf = execution_failure(node_id, "host.pair", &reason_code, &composed_detail);
                ExecDispatchOutcome::Failure {
                    reason_code,
                    detail: Some(composed_detail),
                    runtime_failure: Some(rf),
                }
            }
            Ok(StoreDispatchOutcome::InstallReconcile(InstallReconcileOutcome::Reconciled {
                desired_hash,
                applied_steps,
                skipped_steps,
            })) => ExecDispatchOutcome::Success {
                evidence_ref: Some(desired_hash.clone()),
                detail: Some(json!({
                    "slice": "install.reconcile",
                    "desired_hash": desired_hash,
                    "applied_steps": applied_steps,
                    "skipped_steps": skipped_steps,
                })),
            },
            Ok(StoreDispatchOutcome::InstallReconcile(InstallReconcileOutcome::Failed {
                reason_code,
                reason_detail,
                phase,
                applied_steps,
                ..
            })) => {
                let composed_detail = format!("[{phase}] {reason_detail}");
                let rf =
                    execution_failure(node_id, "install.reconcile", &reason_code, &composed_detail);
                ExecDispatchOutcome::Failure {
                    reason_code,
                    detail: Some(format!("{composed_detail}; applied_steps={applied_steps}")),
                    runtime_failure: Some(rf),
                }
            }
            Err(StoreError::Contract(msg)) if msg.contains("unsupported operational command") => {
                // Honest admission: dispatcher contract doesn't know this
                // verb / target. No material act attempted.
                ExecDispatchOutcome::Blocked { reason: msg }
            }
            Err(StoreError::Contract(msg)) => {
                // Any other Contract error (e.g. malformed args) is a
                // pre-dispatch refusal, not an in-flight failure.
                ExecDispatchOutcome::Blocked { reason: msg }
            }
            Err(other) => {
                let reason_code = store_error_reason_code(&other);
                let detail = other.to_string();
                let rf = execution_failure(
                    node_id,
                    &format!("{}.{}", command.namespace, command.verb),
                    reason_code,
                    &detail,
                );
                ExecDispatchOutcome::Failure {
                    reason_code: reason_code.into(),
                    detail: Some(detail),
                    runtime_failure: Some(rf),
                }
            }
        }
    }
}

/// Compose the canonical [`RuntimeFailure::Execution`] the dispatcher
/// attaches whenever a slice outcome crosses the `Failure` boundary. Every
/// failure path goes through this helper so the stage, surface, and
/// reason_code discipline is uniform at the dispatch boundary.
fn execution_failure(
    node_id: &NodeId,
    surface: &str,
    reason_code: &str,
    detail: &str,
) -> RuntimeFailure {
    RuntimeFailure::Execution {
        command_id: Some(node_id.0.clone()),
        surface: surface.to_string(),
        reason_code: reason_code.to_string(),
        detail: detail.to_string(),
    }
}

fn store_error_reason_code(err: &StoreError) -> &'static str {
    match err {
        StoreError::Http(_) => "http_error",
        StoreError::Supabase { .. } => "supabase_error",
        StoreError::Env(_) => "env_missing",
        StoreError::InvalidEnv { .. } => "env_invalid",
        StoreError::Provider { .. } => "provider_error",
        StoreError::Contract(_) => "contract_error",
        StoreError::SendBlocked { .. } => "send_blocked",
    }
}

impl AsyncDispatcher for RealDispatcher {
    fn dispatch_async<'a>(
        &'a self,
        node_id: &'a NodeId,
        command: &'a OperationalCommand,
    ) -> Pin<Box<dyn Future<Output = ExecDispatchOutcome> + Send + 'a>> {
        Box::pin(self.dispatch_real(node_id, command))
    }
}

#[cfg(test)]
mod tests {
    //! Routing-layer unit tests. End-to-end tests that actually drive the
    //! live slice orchestrators live in `tests/real_dispatcher.rs` so they
    //! can reuse the existing wiremock seeding infrastructure and stay
    //! aligned with the existing slice tests.

    use super::*;
    use constitutional_runtime::RuntimeTarget;
    use minilab_core::SimMode;
    use std::collections::BTreeMap;

    fn client() -> StoreClient {
        StoreClient::with_mode("https://example.invalid", "k", SimMode::Simulation)
    }

    #[tokio::test]
    async fn unknown_command_becomes_blocked_not_failure() {
        let dispatcher = RealDispatcher::new(client());
        let command = OperationalCommand {
            namespace: "unknown".into(),
            verb: "act".into(),
            args: BTreeMap::new(),
            target_runtime: RuntimeTarget::Platform,
        };
        let out = dispatcher
            .dispatch_real(&NodeId("n0".into()), &command)
            .await;
        match out {
            ExecDispatchOutcome::Blocked { reason } => {
                assert!(
                    reason.contains("unsupported operational command"),
                    "reason={reason}"
                );
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execution_failure_carries_runtime_failure_tagged_at_execution_stage() {
        // Point the client at an unroutable URL so the underlying dispatch
        // path produces a `StoreError::Http`, which the adapter must map
        // to `Failure { runtime_failure: Some(RuntimeFailure::Execution { .. }) }`.
        let bad_client =
            StoreClient::with_mode("http://127.0.0.1:1/", "service-key", SimMode::Simulation);
        let dispatcher = RealDispatcher::new(bad_client);

        // We use a triple the dispatcher *does* route (outbound.send / Provider)
        // so the failure happens in-flight, not at the routing gate.
        let mut args = std::collections::BTreeMap::new();
        args.insert("contact_id".into(), serde_json::json!(uuid::Uuid::nil()));
        args.insert("campaign_id".into(), serde_json::json!(uuid::Uuid::nil()));
        args.insert("channel".into(), serde_json::json!("email"));
        args.insert(
            "correlation_id".into(),
            serde_json::json!(uuid::Uuid::nil()),
        );
        let command = OperationalCommand {
            namespace: "outbound".into(),
            verb: "send".into(),
            args,
            target_runtime: RuntimeTarget::Provider,
        };

        let out = dispatcher
            .dispatch_real(&NodeId("n7".into()), &command)
            .await;
        match out {
            ExecDispatchOutcome::Failure {
                reason_code,
                runtime_failure,
                ..
            } => {
                assert_eq!(reason_code, "http_error");
                let rf = runtime_failure.expect("PR 5c: RuntimeFailure must be attached");
                // Stage tag is closed + matches the dispatch boundary.
                assert_eq!(rf.stage().as_str(), "execution");
                // reason_code agrees across the struct and the structured form.
                assert_eq!(rf.reason_code(), "http_error");
                if let RuntimeFailure::Execution {
                    command_id,
                    surface,
                    ..
                } = rf
                {
                    assert_eq!(command_id.as_deref(), Some("n7"));
                    assert_eq!(surface, "outbound.send");
                } else {
                    panic!("expected RuntimeFailure::Execution variant");
                }
            }
            other => panic!("expected Failure with runtime_failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn outbound_send_with_wrong_target_runtime_is_blocked() {
        // The live dispatcher matches on (namespace, verb, target_runtime);
        // the tuple mismatch produces a Contract error, which we promote to
        // Blocked rather than Failure.
        let dispatcher = RealDispatcher::new(client());
        let command = OperationalCommand {
            namespace: "outbound".into(),
            verb: "send".into(),
            args: BTreeMap::new(),
            target_runtime: RuntimeTarget::Shell, // wrong target
        };
        let out = dispatcher
            .dispatch_real(&NodeId("n0".into()), &command)
            .await;
        assert!(matches!(out, ExecDispatchOutcome::Blocked { .. }));
    }
}
