//! Runtime failure normalization.
//!
//! Every failure the constitutional runtime can produce is classified into one
//! of a small, closed set of stages. This is the Phase-1 contract from the
//! transplant plan: failure is not free-form text, it is a **stage-labelled
//! runtime object** that any downstream component (evidence writer, planner,
//! agent-runtime reply composer) can match on without string-parsing.
//!
//! The seven stages mirror the constitutional pipeline (see
//! `docs/integration/slice-pattern.md` §2):
//!
//!   Parse → Validation → Policy → Capability → Lowering → Execution → Evidence
//!
//! Every stage produces a typed variant. Additional per-domain failure
//! vocabularies (e.g. `replay_detected`, `token_mismatch` for the host.pair
//! slice) are carried as structured `reason_code` strings inside the
//! appropriate variant — the vocabulary is open, but the **stage** is closed.
//!
//! Invariants:
//!
//! - A failure carries the `stage` it was raised at, so downstream code can
//!   distinguish a `Policy` rejection from an `Execution` error without
//!   re-deriving it from other fields.
//! - Evidence rows for failed constitutional acts record `phase` on the
//!   payload (`pre_admission` or `execution`). This enum is the authoritative
//!   Rust-side mirror of that phase discipline: parse/validation/policy/
//!   capability/lowering map to `pre_admission`; execution/evidence map to
//!   `execution`. See [`FailureStage::phase`].

use serde::{Deserialize, Serialize};

/// Closed set of pipeline stages a failure can be raised at.
///
/// This is the canonical vocabulary the transplant plan mandates: runtime
/// failures must be classifiable by stage, not only by message.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    /// Input failed to parse (strong grammar or operational grammar text).
    Parse,
    /// Parsed input was structurally invalid (missing required field,
    /// enum out of range, envelope shape broken).
    Validation,
    /// Policy envelope rejected the act (class A/B/C admission).
    Policy,
    /// No substrate advertised a capability sufficient to realize the act.
    Capability,
    /// Lowering compiler could not produce an `OperationalCommand` (e.g. no
    /// matching lowerer arm, target runtime refused the shape).
    Lowering,
    /// The substrate refused or failed the executed command (network,
    /// upstream error, simulation escape, missing real executor).
    Execution,
    /// The evidence store could not record the outcome durably.
    Evidence,
}

impl FailureStage {
    /// Which admission phase this stage belongs to. Mirrors the `phase`
    /// field on constitutional `*.failed` evidence rows.
    pub fn phase(self) -> FailurePhase {
        match self {
            FailureStage::Parse
            | FailureStage::Validation
            | FailureStage::Policy
            | FailureStage::Capability
            | FailureStage::Lowering => FailurePhase::PreAdmission,
            FailureStage::Execution | FailureStage::Evidence => FailurePhase::Execution,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FailureStage::Parse => "parse",
            FailureStage::Validation => "validation",
            FailureStage::Policy => "policy",
            FailureStage::Capability => "capability",
            FailureStage::Lowering => "lowering",
            FailureStage::Execution => "execution",
            FailureStage::Evidence => "evidence",
        }
    }
}

/// Admission-level bucket. Maps 1:1 onto the `phase` payload field written
/// by the evidence ledger for `*.failed` rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePhase {
    PreAdmission,
    Execution,
}

impl FailurePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            FailurePhase::PreAdmission => "pre_admission",
            FailurePhase::Execution => "execution",
        }
    }
}

/// Canonical runtime failure.
///
/// Every variant carries at minimum enough context to route the failure to
/// the right evidence shape and to compose a user-legible reply. The exact
/// structured bodies are additive — they grow as new slices need them — but
/// the stage discriminant is closed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum RuntimeFailure {
    Parse {
        /// Which grammar was being parsed (`"strong"`, `"operational"`).
        grammar: String,
        /// Human-scale detail — do not parse this; use `reason_code`.
        detail: String,
        /// Stable machine-readable code (e.g. `"unknown_kind"`,
        /// `"missing_required_arg"`).
        reason_code: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    Validation {
        /// Logical stage of the pipeline that raised the validation error
        /// (e.g. `"ir"`, `"operational_ast"`, `"strong_ast"`).
        at: String,
        /// Optional dotted field path (e.g. `"execute.params.contact_id"`).
        field: Option<String>,
        detail: String,
        reason_code: String,
    },
    Policy {
        /// Policy class that rejected the act (A/B/C).
        class: PolicyClassTag,
        /// Machine-readable reason (e.g. `"opt_out"`, `"rate_limited"`).
        reason_code: String,
        detail: String,
    },
    Capability {
        /// Primitive we tried to realize, as the `PrimitiveName` string.
        primitive: String,
        /// Kind filter if applicable (e.g. `"outbound.send"`).
        kind: Option<String>,
        /// Substrate we attempted, if any was selected.
        attempted_substrate: Option<String>,
        reason_code: String,
        detail: String,
    },
    Lowering {
        /// IR node id for cross-referencing with the validated graph.
        node_id: String,
        target_runtime: String,
        reason_code: String,
        detail: String,
    },
    Execution {
        /// Identifier of the dispatched operational command.
        command_id: Option<String>,
        /// Substrate surface that ran the command (e.g. `"sendgrid"`,
        /// `"platform-mock"`).
        surface: String,
        reason_code: String,
        detail: String,
    },
    Evidence {
        /// Node or evidence row this write was trying to close.
        node_id: Option<String>,
        reason_code: String,
        detail: String,
    },
}

/// Tag for policy class. Does not replace the richer [`crate::policy::PolicyClass`];
/// it is a string-safe, serde-stable discriminant suitable for failure payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PolicyClassTag {
    A,
    B,
    C,
}

impl RuntimeFailure {
    pub fn stage(&self) -> FailureStage {
        match self {
            RuntimeFailure::Parse { .. } => FailureStage::Parse,
            RuntimeFailure::Validation { .. } => FailureStage::Validation,
            RuntimeFailure::Policy { .. } => FailureStage::Policy,
            RuntimeFailure::Capability { .. } => FailureStage::Capability,
            RuntimeFailure::Lowering { .. } => FailureStage::Lowering,
            RuntimeFailure::Execution { .. } => FailureStage::Execution,
            RuntimeFailure::Evidence { .. } => FailureStage::Evidence,
        }
    }

    pub fn phase(&self) -> FailurePhase {
        self.stage().phase()
    }

    /// Stable machine-readable reason code. Never empty for a well-formed
    /// failure — the constructor sites are responsible.
    pub fn reason_code(&self) -> &str {
        match self {
            RuntimeFailure::Parse { reason_code, .. }
            | RuntimeFailure::Validation { reason_code, .. }
            | RuntimeFailure::Policy { reason_code, .. }
            | RuntimeFailure::Capability { reason_code, .. }
            | RuntimeFailure::Lowering { reason_code, .. }
            | RuntimeFailure::Execution { reason_code, .. }
            | RuntimeFailure::Evidence { reason_code, .. } => reason_code,
        }
    }
}

impl std::fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RuntimeFailure[{}/{}]: {}",
            self.stage().as_str(),
            self.reason_code(),
            match self {
                RuntimeFailure::Parse { detail, .. }
                | RuntimeFailure::Validation { detail, .. }
                | RuntimeFailure::Policy { detail, .. }
                | RuntimeFailure::Capability { detail, .. }
                | RuntimeFailure::Lowering { detail, .. }
                | RuntimeFailure::Execution { detail, .. }
                | RuntimeFailure::Evidence { detail, .. } => detail,
            }
        )
    }
}

impl std::error::Error for RuntimeFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_maps_to_the_correct_phase() {
        assert_eq!(FailureStage::Parse.phase(), FailurePhase::PreAdmission);
        assert_eq!(FailureStage::Validation.phase(), FailurePhase::PreAdmission);
        assert_eq!(FailureStage::Policy.phase(), FailurePhase::PreAdmission);
        assert_eq!(FailureStage::Capability.phase(), FailurePhase::PreAdmission);
        assert_eq!(FailureStage::Lowering.phase(), FailurePhase::PreAdmission);
        assert_eq!(FailureStage::Execution.phase(), FailurePhase::Execution);
        assert_eq!(FailureStage::Evidence.phase(), FailurePhase::Execution);
    }

    #[test]
    fn phase_strings_match_the_evidence_payload_vocabulary() {
        // These exact strings are what live slices (host.pair, outbound.send)
        // write into `evidence_ledger.payload.phase`. Changing them is a
        // constitutional break.
        assert_eq!(FailurePhase::PreAdmission.as_str(), "pre_admission");
        assert_eq!(FailurePhase::Execution.as_str(), "execution");
    }

    #[test]
    fn runtime_failure_roundtrips_and_exposes_stage_and_reason_code() {
        let f = RuntimeFailure::Capability {
            primitive: "EXECUTE".into(),
            kind: Some("outbound.send".into()),
            attempted_substrate: Some("sendgrid".into()),
            reason_code: "no_binding".into(),
            detail: "no capability binding for EXECUTE outbound.send".into(),
        };
        assert_eq!(f.stage(), FailureStage::Capability);
        assert_eq!(f.phase(), FailurePhase::PreAdmission);
        assert_eq!(f.reason_code(), "no_binding");

        let s = serde_json::to_string(&f).unwrap();
        let back: RuntimeFailure = serde_json::from_str(&s).unwrap();
        assert_eq!(f, back);

        // Must be externally tagged by `stage` so ledger consumers can
        // discriminate without heuristic.
        assert!(s.contains(r#""stage":"capability""#));
    }

    #[test]
    fn execution_failures_surface_substrate_and_reason_code() {
        let f = RuntimeFailure::Execution {
            command_id: Some("cmd-123".into()),
            surface: "platform-mock".into(),
            reason_code: "upstream_error".into(),
            detail: "missing real platform executor".into(),
        };
        assert_eq!(f.phase(), FailurePhase::Execution);
        assert_eq!(f.reason_code(), "upstream_error");
        let rendered = format!("{f}");
        assert!(rendered.contains("execution/upstream_error"));
    }

    #[test]
    fn display_format_is_stage_slash_reason_then_detail() {
        let f = RuntimeFailure::Policy {
            class: PolicyClassTag::B,
            reason_code: "opt_out".into(),
            detail: "contact opted out".into(),
        };
        assert_eq!(
            format!("{f}"),
            "RuntimeFailure[policy/opt_out]: contact opted out"
        );
    }
}
