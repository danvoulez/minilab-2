//! Canonical runtime dispatch for lowered `OperationalCommand`s.
//!
//! This module closes the last formal gap between the landed slices and the
//! runtime's live path:
//!
//! ```text
//!   IR Execute node
//!     -> MinilabRuntimeLowerer
//!     -> OperationalCommand
//!     -> dispatch_operational_command
//!     -> slice orchestrator
//! ```
//!
//! The dispatcher is intentionally thin. It does not re-implement
//! admissibility or execution semantics; it only:
//!
//! - accepts the canonical lowered command;
//! - checks the namespace / verb / target_runtime contract; and
//! - reconstructs the minimal typed input expected by the slice orchestrator.
//!
//! Anything outside the two landed constitutional acts (`outbound.send`,
//! `host.pair`) is rejected explicitly with `StoreError::Contract`.

use std::collections::BTreeMap;

use constitutional_runtime::{
    ActionKind, CanonicalActionId, IRPrimitive, IrNode, Lowerer, LoweringPlan,
    MinilabRuntimeLowerer, NodeId, OperationalCommand, RuntimeTarget,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::host_pair::{submit_host_pair, HostPairInput, HostPairOutcome};
use crate::install_reconcile::{
    submit_install_reconcile, InstallReconcileInput, InstallReconcileOutcome,
};
use crate::outbound_orchestrator::{submit_outbound_send, OutboundSendInput, OutboundSendOutcome};

/// Terminal result of dispatching one canonical lowered command.
#[derive(Debug, Clone)]
pub enum DispatchOutcome {
    OutboundSend(OutboundSendOutcome),
    HostPair(HostPairOutcome),
    InstallReconcile(InstallReconcileOutcome),
}

impl DispatchOutcome {
    pub fn into_outbound_send(self) -> Result<OutboundSendOutcome, StoreError> {
        match self {
            Self::OutboundSend(outcome) => Ok(outcome),
            Self::HostPair(_) => Err(StoreError::Contract(
                "dispatcher returned host.pair outcome for outbound.send caller".into(),
            )),
            Self::InstallReconcile(_) => Err(StoreError::Contract(
                "dispatcher returned install.reconcile outcome for outbound.send caller".into(),
            )),
        }
    }

    pub fn into_host_pair(self) -> Result<HostPairOutcome, StoreError> {
        match self {
            Self::HostPair(outcome) => Ok(outcome),
            Self::OutboundSend(_) => Err(StoreError::Contract(
                "dispatcher returned outbound.send outcome for host.pair caller".into(),
            )),
            Self::InstallReconcile(_) => Err(StoreError::Contract(
                "dispatcher returned install.reconcile outcome for host.pair caller".into(),
            )),
        }
    }

    pub fn into_install_reconcile(self) -> Result<InstallReconcileOutcome, StoreError> {
        match self {
            Self::InstallReconcile(outcome) => Ok(outcome),
            Self::OutboundSend(_) => Err(StoreError::Contract(
                "dispatcher returned outbound.send outcome for install.reconcile caller".into(),
            )),
            Self::HostPair(_) => Err(StoreError::Contract(
                "dispatcher returned host.pair outcome for install.reconcile caller".into(),
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutboundSendCommandArgs {
    contact_id: Uuid,
    campaign_id: Uuid,
    channel: String,
    correlation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostPairCommandArgs {
    host_id: Uuid,
    challenge: String,
    agent_pubkey: String,
    correlation_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallReconcileCommandArgs {
    installation_id: Uuid,
    host_id: Uuid,
    desired_manifest: Value,
    #[serde(default)]
    applied_manifest: Option<Value>,
    correlation_id: Uuid,
}

/// Lower a single `Execute(<action>)` node into its canonical command.
pub fn lower_execute_action(
    node_name: &str,
    action_name: &str,
    params: BTreeMap<String, Value>,
) -> Result<(LoweringPlan, OperationalCommand), StoreError> {
    let action = CanonicalActionId::parse(action_name).map_err(|err| {
        StoreError::Contract(format!(
            "failed to parse execute action `{action_name}` as canonical identity: {err}"
        ))
    })?;
    let node = IrNode {
        id: NodeId(node_name.into()),
        body: IRPrimitive::Execute {
            action: ActionKind::Canonical(action),
            params: params.into_iter().collect::<Map<String, Value>>(),
        },
    };
    let lowerer = MinilabRuntimeLowerer;
    lowerer.lower(&node).map_err(|err| {
        StoreError::Contract(format!(
            "failed to lower execute action `{action_name}`: {err}"
        ))
    })
}

/// Convenience helper for the live path used by API surfaces and tests that
/// want the full canonical flow in one call.
pub async fn lower_and_dispatch_execute(
    client: &StoreClient,
    node_name: &str,
    action_name: &str,
    params: BTreeMap<String, Value>,
) -> Result<DispatchOutcome, StoreError> {
    let (_, command) = lower_execute_action(node_name, action_name, params)?;
    dispatch_operational_command(client, command).await
}

/// Route a canonical lowered command into the matching slice orchestrator.
pub async fn dispatch_operational_command(
    client: &StoreClient,
    command: OperationalCommand,
) -> Result<DispatchOutcome, StoreError> {
    match (
        command.namespace.as_str(),
        command.verb.as_str(),
        &command.target_runtime,
    ) {
        ("outbound", "send", RuntimeTarget::Provider) => {
            let args: OutboundSendCommandArgs = decode_args(&command)?;
            let outcome = submit_outbound_send(
                client,
                OutboundSendInput {
                    contact_id: args.contact_id,
                    campaign_id: args.campaign_id,
                    channel: args.channel,
                    correlation_id: args.correlation_id,
                },
            )
            .await?;
            Ok(DispatchOutcome::OutboundSend(outcome))
        }
        ("host", "pair", RuntimeTarget::Platform) => {
            let args: HostPairCommandArgs = decode_args(&command)?;
            let outcome = submit_host_pair(
                client,
                HostPairInput {
                    host_id: args.host_id,
                    challenge: args.challenge,
                    agent_pubkey: args.agent_pubkey,
                    correlation_id: args.correlation_id,
                },
            )
            .await?;
            Ok(DispatchOutcome::HostPair(outcome))
        }
        ("install", "reconcile", RuntimeTarget::Platform) => {
            let args: InstallReconcileCommandArgs = decode_args(&command)?;
            let outcome = submit_install_reconcile(
                client,
                InstallReconcileInput {
                    installation_id: args.installation_id,
                    host_id: args.host_id,
                    desired_manifest: args.desired_manifest,
                    applied_manifest: args.applied_manifest,
                    correlation_id: args.correlation_id,
                },
            )
            .await?;
            Ok(DispatchOutcome::InstallReconcile(outcome))
        }
        _ => Err(StoreError::Contract(format!(
            "unsupported operational command for dispatcher: {}.{} target_runtime={:?}",
            command.namespace, command.verb, command.target_runtime
        ))),
    }
}

fn decode_args<T: DeserializeOwned>(command: &OperationalCommand) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::to_value(&command.args).map_err(|err| {
        StoreError::Contract(format!(
            "failed to serialize command args for {}.{}: {err}",
            command.namespace, command.verb
        ))
    })?)
    .map_err(|err| {
        StoreError::Contract(format!(
            "invalid command args for {}.{}: {err}",
            command.namespace, command.verb
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{dispatch_operational_command, lower_execute_action};
    use crate::client::StoreClient;
    use constitutional_runtime::RuntimeTarget;
    use minilab_core::SimMode;
    use serde_json::json;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    #[test]
    fn lower_execute_action_builds_provider_target_for_outbound_send() {
        let mut params = BTreeMap::new();
        params.insert("contact_id".into(), json!(Uuid::nil()));
        params.insert("campaign_id".into(), json!(Uuid::nil()));
        params.insert("channel".into(), json!("email"));
        params.insert("correlation_id".into(), json!(Uuid::nil()));

        let (_, command) =
            lower_execute_action("dispatch-outbound", "outbound.send", params).unwrap();
        assert_eq!(command.namespace, "outbound");
        assert_eq!(command.verb, "send");
        assert_eq!(command.target_runtime, RuntimeTarget::Provider);
    }

    #[tokio::test]
    async fn dispatcher_rejects_unknown_command_contractually() {
        let client = StoreClient::with_mode(
            "https://example.supabase.co",
            "service-key",
            SimMode::Simulation,
        );
        let mut params = BTreeMap::new();
        params.insert("correlation_id".into(), json!(Uuid::nil()));
        let command = constitutional_runtime::OperationalCommand {
            namespace: "unknown".into(),
            verb: "act".into(),
            args: params,
            target_runtime: RuntimeTarget::Platform,
        };

        let err = dispatch_operational_command(&client, command)
            .await
            .expect_err("unknown commands must be rejected");
        assert!(err.to_string().contains("unsupported operational command"));
    }
}
