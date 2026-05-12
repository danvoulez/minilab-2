pub mod admission;
pub mod audit;
pub mod campaign;
pub mod client;
pub mod dispatcher;
pub mod eligibility;
pub mod evidence;
pub mod host_pair;
pub mod install_reconcile;
pub mod optout_gate;
pub mod outbound;
pub mod outbound_orchestrator;
pub mod outreach;
pub mod policy;
pub mod premium;
pub mod real_dispatcher;
pub mod reply;
pub mod scoring;
pub mod store;
pub mod webhook;

pub use admission::{
    AdmissionCommitReceipt, AdmissionService, AdmissionWorld, InMemoryAdmissionState,
    InMemoryAdmissionWorld, SupabaseAdmissionWorld,
};
pub use audit::{
    export_evidence_trail, kpi_snapshot, EvidenceTrail, EvidenceTrailRow, EvidenceTrailSummary,
    KpiSnapshot, TouchExplanation,
};
pub use client::{StoreClient, StoreError};
pub use dispatcher::{
    dispatch_operational_command, lower_and_dispatch_execute, lower_execute_action, DispatchOutcome,
};
pub use host_pair::{submit_host_pair, HostPairInput, HostPairOutcome};
pub use install_reconcile::{
    submit_install_reconcile, InstallReconcileInput, InstallReconcileOutcome,
};
pub use outbound_orchestrator::{submit_outbound_send, OutboundSendInput, OutboundSendOutcome};
pub use store::{ingest_signal, IngestInput, IngestOutput};
