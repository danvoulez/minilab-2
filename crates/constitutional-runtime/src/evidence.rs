//! Evidence contract, records, store trait, and closure semantics.

use crate::lowering::{LoweringPlan, OperationalCommand};
use crate::refs::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

/// Minimum evidence expectations attached to a lowering or execution plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceContract {
    /// Logical evidence kinds that must be present before the act is considered closed.
    pub required_kinds: Vec<String>,
}

/// Append-only record (store shape is product-specific; JSON payload is canonical).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub kind: String,
    pub payload_json: serde_json::Value,
}

impl EvidenceRecord {
    /// Semantic evidence: what was lowered and under which contract.
    pub fn from_plan(node_id: &NodeId, plan: &LoweringPlan, cmd: &OperationalCommand) -> Self {
        EvidenceRecord {
            kind: "lowering.semantic".into(),
            payload_json: json!({
                "node_id": node_id.0,
                "requires_confirmation": plan.requires_confirmation,
                "estimated_latency_ms": plan.estimated_latency_ms,
                "evidence_contract": plan.evidence,
                "operational": {
                    "namespace": cmd.namespace,
                    "verb": cmd.verb,
                    "args": cmd.args,
                    "target_runtime": cmd.target_runtime,
                },
            }),
        }
    }

    /// Execution outcome evidence (success or failure at the muscle layer).
    pub fn from_execution_result(
        node_id: &NodeId,
        success: bool,
        output_ref: Option<&str>,
    ) -> Self {
        EvidenceRecord {
            kind: "exec.result".into(),
            payload_json: json!({
                "node_id": node_id.0,
                "success": success,
                "output_ref": output_ref,
            }),
        }
    }

    /// Closure status: constitutional close vs partial / failed close.
    pub fn from_closure_status(node_id: &NodeId, closed: bool, detail: serde_json::Value) -> Self {
        EvidenceRecord {
            kind: "closure.status".into(),
            payload_json: json!({
                "node_id": node_id.0,
                "constitutionally_closed": closed,
                "detail": detail,
            }),
        }
    }
}

#[derive(Debug, Error)]
pub enum EvidenceStoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization: {0}")]
    Serde(String),
    #[error("sqlite: {0}")]
    Sqlite(String),
    /// HTTP sink (e.g. PostgREST / Supabase REST).
    #[error("http: {0}")]
    Http(String),
}

/// Execution succeeded materially but evidence could not be persisted: not closed.
#[derive(Debug, Error)]
pub enum FailureToClose {
    #[error("evidence write failed after successful execution: {0}")]
    EvidenceWriteFailed(EvidenceStoreError),
}

pub trait EvidenceStore: Send + Sync {
    fn write_record(&self, record: EvidenceRecord) -> Result<(), EvidenceStoreError>;
}

/// Append one JSON line per record. Suitable for local Minilab / audit tail.
pub struct FileEvidenceStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileEvidenceStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            lock: Mutex::new(()),
        }
    }
}

impl EvidenceStore for FileEvidenceStore {
    fn write_record(&self, record: EvidenceRecord) -> Result<(), EvidenceStoreError> {
        let _g = self.lock.lock().expect("evidence store mutex poisoned");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line =
            serde_json::to_string(&record).map_err(|e| EvidenceStoreError::Serde(e.to_string()))?;
        writeln!(f, "{line}")?;
        Ok(())
    }
}

/// Writes all records. If **any** write fails, returns [`FailureToClose`].
/// After a **successful** execution, this is the constitutional gate: evidence failure means not closed.
pub fn close_execution_evidence(
    store: &dyn EvidenceStore,
    records: Vec<EvidenceRecord>,
) -> Result<(), FailureToClose> {
    for r in records {
        store
            .write_record(r)
            .map_err(FailureToClose::EvidenceWriteFailed)?;
    }
    Ok(())
}
