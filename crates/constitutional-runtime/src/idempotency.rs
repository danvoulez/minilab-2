//! Idempotency contracts as first-class runtime types.
//!
//! The Minilab Formal Spec requires idempotency to be a **declared property**
//! of every act, not an implementation quirk of each substrate. This module
//! pours the closed set from the canon into Rust:
//!
//!   PureRead · DeterministicRecompute · DeduplicatedMaterialAction · ConfirmedNonIdempotent
//!
//! The class determines what safe behavior looks like on retry, what the
//! planner is allowed to do under failure, and what the evidence ledger
//! should write when the same inputs arrive twice.
//!
//! Design notes:
//!
//! - The class is a **static property of the act**, independent of the
//!   substrate that executes it. A `PureRead` stays pure even if the
//!   substrate happens to cache.
//! - [`IdempotencyContract`] composes the class with a machine-readable
//!   retry stance and an optional idempotency-key shape, so downstream code
//!   (planner, dispatcher, orchestrator) does not re-derive rules from the
//!   class alone.
//! - No runtime behavior changes yet. The types are landed first so Phase 2
//!   (operational grammar) and Phase 3 (planner) can consume them.

use serde::{Deserialize, Serialize};

/// Closed classification of act idempotency behavior.
///
/// This is the vocabulary from the Minilab Formal Spec. Adding a new variant
/// is a constitutional change, not a mechanical one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyClass {
    /// No state-mutating effect. Safe to replay freely; equal inputs yield
    /// equal outputs up to observation latency. `OBSERVE`, `FETCH`,
    /// `COLLECT` live here by default.
    PureRead,
    /// Mutates only derived / cached state that can be recomputed from source
    /// of truth. Retries are safe because divergence is bounded by a
    /// recomputation. `COMPRESS`, `CLASSIFY`, `PRIORITIZE` typically.
    DeterministicRecompute,
    /// Performs a material, externally-visible action (send, pay, pair) and
    /// achieves idempotency by carrying an idempotency key that the
    /// substrate deduplicates on. Retries with the same key are a no-op
    /// success; replays with a new key are a new act.
    DeduplicatedMaterialAction,
    /// Performs a material action that cannot be deduplicated — the act is
    /// inherently once-only and requires explicit human/authority
    /// confirmation. No retry policy applies; the runtime must escalate on
    /// failure rather than retry silently.
    ConfirmedNonIdempotent,
}

impl IdempotencyClass {
    pub fn as_str(self) -> &'static str {
        match self {
            IdempotencyClass::PureRead => "pure_read",
            IdempotencyClass::DeterministicRecompute => "deterministic_recompute",
            IdempotencyClass::DeduplicatedMaterialAction => "deduplicated_material_action",
            IdempotencyClass::ConfirmedNonIdempotent => "confirmed_non_idempotent",
        }
    }

    /// Whether a mechanical retry (no human confirmation) is permitted on
    /// transient failure. The planner and dispatcher must honor this.
    pub fn mechanical_retry_allowed(self) -> bool {
        match self {
            IdempotencyClass::PureRead
            | IdempotencyClass::DeterministicRecompute
            | IdempotencyClass::DeduplicatedMaterialAction => true,
            IdempotencyClass::ConfirmedNonIdempotent => false,
        }
    }

    /// Whether an idempotency key is semantically required for the class.
    /// `DeduplicatedMaterialAction` is the only class that demands one.
    pub fn requires_idempotency_key(self) -> bool {
        matches!(self, IdempotencyClass::DeduplicatedMaterialAction)
    }

    /// Whether repeated submissions with equal inputs must produce a
    /// single material effect (i.e. the runtime is responsible for dedupe).
    pub fn enforces_single_effect(self) -> bool {
        matches!(
            self,
            IdempotencyClass::DeduplicatedMaterialAction | IdempotencyClass::ConfirmedNonIdempotent
        )
    }
}

/// How the runtime should react when the same idempotency key resurfaces.
///
/// Distinct from the class because one class may admit different ledger
/// treatments per slice. Today every landed slice uses `ReplayRejection`
/// for `host.pair` and `SilentSuccess` for `outbound.send`; these are the
/// two named options.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStance {
    /// A duplicate submission is returned as a no-op success — the prior
    /// outcome is replayed to the caller. Typical for provider sends that
    /// are already idempotent at the vendor.
    SilentSuccess,
    /// A duplicate submission is an explicit, evidence-emitting failure
    /// (e.g. `*.failed { reason_code: "replay_detected" }`). The caller
    /// learns that the prior act already happened.
    ReplayRejection,
    /// The class forbids mechanical replay handling; escalate to a human
    /// or authority-gated path. Used by `ConfirmedNonIdempotent`.
    EscalateToConfirm,
}

/// Full idempotency contract for a single constitutional act.
///
/// Composed in the IR (or declared by the act's lowerer arm) and then carried
/// through the pipeline. The planner uses the `class` to decide retries; the
/// dispatcher uses `key_shape` to build the per-request idempotency key; the
/// orchestrator uses `replay_stance` to decide how to react to a duplicate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyContract {
    pub class: IdempotencyClass,
    pub replay_stance: ReplayStance,
    /// Free-form description of the key shape (e.g. `"sha256(correlation_id || kind)"`).
    /// `None` is legal for classes that do not require a key.
    #[serde(default)]
    pub key_shape: Option<String>,
    /// Machine-readable tags the planner can match on (e.g.
    /// `"provider_managed"`, `"runtime_managed"`). Opaque vocabulary.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl IdempotencyContract {
    /// Convenience constructor for the `PureRead` default.
    pub fn pure_read() -> Self {
        Self {
            class: IdempotencyClass::PureRead,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: None,
            tags: Vec::new(),
        }
    }

    /// Convenience constructor for the `DeterministicRecompute` default.
    pub fn deterministic_recompute() -> Self {
        Self {
            class: IdempotencyClass::DeterministicRecompute,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: None,
            tags: Vec::new(),
        }
    }

    /// Enforces [`IdempotencyClass::requires_idempotency_key`] semantically
    /// — returns false if the contract is invalid.
    pub fn is_wellformed(&self) -> bool {
        if self.class.requires_idempotency_key() && self.key_shape.is_none() {
            return false;
        }
        if matches!(self.class, IdempotencyClass::ConfirmedNonIdempotent)
            && !matches!(self.replay_stance, ReplayStance::EscalateToConfirm)
        {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_string_vocabulary_is_stable() {
        // These strings are the canonical serialization. Changing any is
        // a cross-runtime break (evidence payloads, planner configs,
        // external manifests reference them).
        assert_eq!(IdempotencyClass::PureRead.as_str(), "pure_read");
        assert_eq!(
            IdempotencyClass::DeterministicRecompute.as_str(),
            "deterministic_recompute"
        );
        assert_eq!(
            IdempotencyClass::DeduplicatedMaterialAction.as_str(),
            "deduplicated_material_action"
        );
        assert_eq!(
            IdempotencyClass::ConfirmedNonIdempotent.as_str(),
            "confirmed_non_idempotent"
        );
    }

    #[test]
    fn mechanical_retry_is_only_forbidden_for_confirmed_non_idempotent() {
        assert!(IdempotencyClass::PureRead.mechanical_retry_allowed());
        assert!(IdempotencyClass::DeterministicRecompute.mechanical_retry_allowed());
        assert!(IdempotencyClass::DeduplicatedMaterialAction.mechanical_retry_allowed());
        assert!(!IdempotencyClass::ConfirmedNonIdempotent.mechanical_retry_allowed());
    }

    #[test]
    fn only_deduplicated_material_action_requires_a_key() {
        assert!(!IdempotencyClass::PureRead.requires_idempotency_key());
        assert!(!IdempotencyClass::DeterministicRecompute.requires_idempotency_key());
        assert!(IdempotencyClass::DeduplicatedMaterialAction.requires_idempotency_key());
        assert!(!IdempotencyClass::ConfirmedNonIdempotent.requires_idempotency_key());
    }

    #[test]
    fn contract_wellformedness_rejects_missing_key_for_dedup_class() {
        let bad = IdempotencyContract {
            class: IdempotencyClass::DeduplicatedMaterialAction,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: None,
            tags: vec![],
        };
        assert!(!bad.is_wellformed());

        let good = IdempotencyContract {
            class: IdempotencyClass::DeduplicatedMaterialAction,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: Some("sha256(correlation_id || kind)".into()),
            tags: vec![],
        };
        assert!(good.is_wellformed());
    }

    #[test]
    fn contract_wellformedness_requires_escalate_for_confirmed_class() {
        let bad = IdempotencyContract {
            class: IdempotencyClass::ConfirmedNonIdempotent,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: None,
            tags: vec![],
        };
        assert!(!bad.is_wellformed());

        let good = IdempotencyContract {
            class: IdempotencyClass::ConfirmedNonIdempotent,
            replay_stance: ReplayStance::EscalateToConfirm,
            key_shape: None,
            tags: vec![],
        };
        assert!(good.is_wellformed());
    }

    #[test]
    fn contract_roundtrips_through_json() {
        let c = IdempotencyContract {
            class: IdempotencyClass::DeduplicatedMaterialAction,
            replay_stance: ReplayStance::ReplayRejection,
            key_shape: Some("sha256(host_id || challenge)".into()),
            tags: vec!["runtime_managed".into()],
        };
        let s = serde_json::to_string(&c).unwrap();
        let back: IdempotencyContract = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
        // Canonical serialization keys match the documented vocabulary.
        assert!(s.contains(r#""class":"deduplicated_material_action""#));
        assert!(s.contains(r#""replay_stance":"replay_rejection""#));
    }

    #[test]
    fn landed_slice_shapes_are_expressible() {
        // host.pair today is deduplicated-by-challenge with replay rejection.
        let host_pair = IdempotencyContract {
            class: IdempotencyClass::DeduplicatedMaterialAction,
            replay_stance: ReplayStance::ReplayRejection,
            key_shape: Some("sha256(host_id || challenge)".into()),
            tags: vec!["runtime_managed".into()],
        };
        assert!(host_pair.is_wellformed());
        assert!(host_pair.class.enforces_single_effect());

        // outbound.send today is deduplicated by provider idempotency key
        // with silent success on replay.
        let outbound_send = IdempotencyContract {
            class: IdempotencyClass::DeduplicatedMaterialAction,
            replay_stance: ReplayStance::SilentSuccess,
            key_shape: Some("correlation_id".into()),
            tags: vec!["provider_managed".into()],
        };
        assert!(outbound_send.is_wellformed());
    }
}
