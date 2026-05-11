//! Constitutional `install.reconcile` orchestrator.
//!
//! This is the first Reconcile-shaped slice. Unlike admissibility-only acts
//! (`outbound.send`, `host.pair`), reconcile compares desired and applied
//! state, emits a plan, applies only divergent sub-steps, and closes the act
//! as either reconciled or failed under one `correlation_id`.

use minilab_core::evidence::EvidenceKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReconcileInput {
    pub installation_id: Uuid,
    pub host_id: Uuid,
    pub desired_manifest: Value,
    #[serde(default)]
    pub applied_manifest: Option<Value>,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallReconcileOutcome {
    Reconciled {
        desired_hash: String,
        applied_steps: usize,
        skipped_steps: usize,
    },
    Failed {
        desired_hash: String,
        reason_code: String,
        reason_detail: String,
        phase: &'static str,
        applied_steps: usize,
    },
}

/// Run the Reconcile-shaped slice for an installation manifest.
///
/// Evidence chain:
///
/// 1. `install.reconcile.planned` — desired/applied diff and step list.
/// 2. `install.reconcile.step.applied` — one row for each converging step.
/// 3. `install.reconcile.reconciled` — terminal success.
/// 4. `install.reconcile.failed` — terminal failure at planning/execution.
///
/// `desired_manifest.payload_services[]` is the v0 contract. Each service must
/// carry a stable `id`; when a service also carries `simulate_fail: true`, the
/// mock executor fails at that step after recording any earlier applied steps.
pub async fn submit_install_reconcile(
    client: &StoreClient,
    input: InstallReconcileInput,
) -> Result<InstallReconcileOutcome, StoreError> {
    let desired_hash = stable_hash(&input.desired_manifest)?;

    if prior_reconciled_exists(client, input.installation_id, &desired_hash).await? {
        return Ok(InstallReconcileOutcome::Reconciled {
            desired_hash,
            applied_steps: 0,
            skipped_steps: 0,
        });
    }

    let desired_services = match manifest_services(&input.desired_manifest) {
        Ok(services) => services,
        Err(detail) => {
            return fail(
                client,
                &input,
                &desired_hash,
                "manifest_invalid",
                &detail,
                "planning",
                0,
            )
            .await;
        }
    };

    let applied_services = input
        .applied_manifest
        .as_ref()
        .map(manifest_services)
        .transpose()
        .map_err(StoreError::Contract)?
        .unwrap_or_default();

    let mut steps = Vec::new();
    let mut skipped_steps = 0usize;
    for desired in desired_services {
        if applied_services
            .iter()
            .any(|applied| same_service(applied, &desired))
        {
            skipped_steps += 1;
        } else {
            steps.push(desired);
        }
    }

    insert_ledger_row(
        client,
        EvidenceKind::INSTALL_RECONCILE_PLANNED,
        json!({
            "installation_id": input.installation_id,
            "host_id": input.host_id,
            "desired_hash": &desired_hash,
            "step_count": steps.len(),
            "skipped_steps": skipped_steps,
            "steps": steps.iter().map(service_id).collect::<Vec<_>>(),
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "install.reconcile.planned:{}:{}",
            input.installation_id, desired_hash
        )),
    )
    .await?;

    let mut applied_count = 0usize;
    for (index, step) in steps.iter().enumerate() {
        let step_id = service_id(step);
        if step
            .get("simulate_fail")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return fail(
                client,
                &input,
                &desired_hash,
                "step_failed",
                &format!("install step `{step_id}` failed in simulated executor"),
                "execution",
                applied_count,
            )
            .await;
        }

        insert_ledger_row(
            client,
            EvidenceKind::INSTALL_RECONCILE_STEP_APPLIED,
            json!({
                "installation_id": input.installation_id,
                "host_id": input.host_id,
                "desired_hash": &desired_hash,
                "step_index": index,
                "step_id": &step_id,
                "service": step,
                "correlation_id": input.correlation_id,
            }),
            input.correlation_id,
            Some(format!(
                "install.reconcile.step.applied:{}:{}:{}",
                input.installation_id, desired_hash, step_id
            )),
        )
        .await?;
        applied_count += 1;
    }

    insert_ledger_row(
        client,
        EvidenceKind::INSTALL_RECONCILE_RECONCILED,
        json!({
            "installation_id": input.installation_id,
            "host_id": input.host_id,
            "desired_hash": &desired_hash,
            "applied_steps": applied_count,
            "skipped_steps": skipped_steps,
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "install.reconcile.reconciled:{}:{}",
            input.installation_id, desired_hash
        )),
    )
    .await?;

    Ok(InstallReconcileOutcome::Reconciled {
        desired_hash,
        applied_steps: applied_count,
        skipped_steps,
    })
}

async fn fail(
    client: &StoreClient,
    input: &InstallReconcileInput,
    desired_hash: &str,
    reason_code: &'static str,
    reason_detail: &str,
    phase: &'static str,
    applied_steps: usize,
) -> Result<InstallReconcileOutcome, StoreError> {
    insert_ledger_row(
        client,
        EvidenceKind::INSTALL_RECONCILE_FAILED,
        json!({
            "installation_id": input.installation_id,
            "host_id": input.host_id,
            "desired_hash": &desired_hash,
            "reason_code": reason_code,
            "reason_detail": reason_detail,
            "phase": phase,
            "applied_steps": applied_steps,
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "install.reconcile.failed:{}:{}:{}",
            input.installation_id, desired_hash, reason_code
        )),
    )
    .await?;

    Ok(InstallReconcileOutcome::Failed {
        desired_hash: desired_hash.to_owned(),
        reason_code: reason_code.into(),
        reason_detail: reason_detail.into(),
        phase,
        applied_steps,
    })
}

async fn prior_reconciled_exists(
    client: &StoreClient,
    installation_id: Uuid,
    desired_hash: &str,
) -> Result<bool, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?kind=eq.install.reconcile.reconciled&order=created_at.desc&limit=50",
            client.rest("evidence_ledger")
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    for row in rows {
        let payload = &row["payload"];
        if payload["installation_id"].as_str() == Some(installation_id.to_string().as_str())
            && payload["desired_hash"].as_str() == Some(desired_hash)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn manifest_services(manifest: &Value) -> Result<Vec<Value>, String> {
    let services = manifest
        .get("payload_services")
        .and_then(Value::as_array)
        .ok_or_else(|| "desired_manifest.payload_services must be an array".to_string())?;

    let mut out = Vec::with_capacity(services.len());
    for (index, service) in services.iter().enumerate() {
        let id = service
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if id.trim().is_empty() {
            return Err(format!(
                "payload_services[{index}].id must be a non-empty string"
            ));
        }
        out.push(service.clone());
    }
    Ok(out)
}

fn same_service(left: &Value, right: &Value) -> bool {
    service_id(left) == service_id(right) && left == right
}

fn service_id(service: &Value) -> String {
    service
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string()
}

fn stable_hash(value: &Value) -> Result<String, StoreError> {
    let canonical = serde_json::to_vec(value)
        .map_err(|err| StoreError::Contract(format!("manifest was not serializable: {err}")))?;
    let digest = Sha256::digest(canonical);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{manifest_services, same_service, service_id, stable_hash};
    use serde_json::json;

    #[test]
    fn manifest_services_requires_ids() {
        let err = manifest_services(&json!({"payload_services": [{}]})).unwrap_err();
        assert!(err.contains("id"));
    }

    #[test]
    fn service_comparison_requires_id_and_exact_shape() {
        let a = json!({"id": "api", "version": "1"});
        let b = json!({"id": "api", "version": "1"});
        let c = json!({"id": "api", "version": "2"});
        assert_eq!(service_id(&a), "api");
        assert!(same_service(&a, &b));
        assert!(!same_service(&a, &c));
    }

    #[test]
    fn stable_hash_is_deterministic() {
        let manifest = json!({"payload_services": [{"id": "api"}]});
        assert_eq!(
            stable_hash(&manifest).unwrap(),
            stable_hash(&manifest).unwrap()
        );
    }
}
