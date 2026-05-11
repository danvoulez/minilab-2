//! Capability manifests: material jurisdiction (what a substrate can realize).
//!
//! This module is the **capability-aware lowering** surface that the
//! Minilab Formal Spec §4.2 names: the IR stays substrate-neutral, and a
//! manifest declares which primitives/kinds a given substrate can realize
//! and under what guarantee, cost, latency, and evidence envelopes.
//!
//! The minimal shape landed first (`supported_primitives`,
//! `supported_kinds`, and `declared_guarantees`) stays the runtime-facing
//! contract for the existing validation/lowering path. The richer envelope
//! types below are added as first-class runtime contracts per the transplant
//! plan Phase 1, ahead of the planning/routing compiler that will consume
//! them.
//!
//! Backward compatibility: new envelope fields default to empty and are
//! ignored by the current lowering path. They become load-bearing when the
//! planning compiler lands (Phase 3 / PR 3).

use crate::act_identity::CanonicalActionId;
use crate::ir::IRPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Declared support for IR primitives on a specific substrate (runtime, MCP, shell, …).
///
/// Minimal fields drive realizability checks today. `bindings` carries the
/// richer capability-aware lowering contract — each binding names a primitive
/// (with an optional kind filter) and the guarantee/cost/latency/evidence
/// envelopes that executing through this substrate will yield.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub substrate_id: String,
    pub substrate_version: String,
    /// Which IR primitives this substrate can execute or assist.
    pub supported_primitives: BTreeSet<PrimitiveName>,
    /// If empty, any `Kind` is accepted. If non-empty, `kind` on primitive must match.
    #[serde(default)]
    pub supported_kinds: BTreeSet<String>,
    /// Tags this substrate claims (e.g. `evidence.write`, `append_only`). Used for realizability checks.
    #[serde(default)]
    pub declared_guarantees: BTreeSet<String>,
    /// Per-primitive capability bindings. Consumed by the planning/routing
    /// compiler (Phase 3 of the transplant plan) to select a substrate based
    /// on guarantee, cost, latency, and evidence envelopes.
    #[serde(default)]
    pub bindings: Vec<CapabilityBinding>,
}

/// Stable name for [`crate::ir::IRPrimitive`] variants (for manifest matching).
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrimitiveName {
    #[default]
    Observe,
    Collect,
    Fetch,
    Compress,
    Classify,
    Prioritize,
    Compare,
    Decide,
    Route,
    Schedule,
    Execute,
    Reconcile,
    Emit,
    Persist,
    Confirm,
    Cancel,
}

impl PrimitiveName {
    pub fn from_primitive(p: &IRPrimitive) -> Self {
        match p {
            IRPrimitive::Observe { .. } => PrimitiveName::Observe,
            IRPrimitive::Collect { .. } => PrimitiveName::Collect,
            IRPrimitive::Fetch { .. } => PrimitiveName::Fetch,
            IRPrimitive::Compress { .. } => PrimitiveName::Compress,
            IRPrimitive::Classify { .. } => PrimitiveName::Classify,
            IRPrimitive::Prioritize { .. } => PrimitiveName::Prioritize,
            IRPrimitive::Compare { .. } => PrimitiveName::Compare,
            IRPrimitive::Decide { .. } => PrimitiveName::Decide,
            IRPrimitive::Route { .. } => PrimitiveName::Route,
            IRPrimitive::Schedule { .. } => PrimitiveName::Schedule,
            IRPrimitive::Execute { .. } => PrimitiveName::Execute,
            IRPrimitive::Reconcile { .. } => PrimitiveName::Reconcile,
            IRPrimitive::Emit { .. } => PrimitiveName::Emit,
            IRPrimitive::Persist { .. } => PrimitiveName::Persist,
            IRPrimitive::Confirm { .. } => PrimitiveName::Confirm,
            IRPrimitive::Cancel { .. } => PrimitiveName::Cancel,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PrimitiveName::Observe => "OBSERVE",
            PrimitiveName::Collect => "COLLECT",
            PrimitiveName::Fetch => "FETCH",
            PrimitiveName::Compress => "COMPRESS",
            PrimitiveName::Classify => "CLASSIFY",
            PrimitiveName::Prioritize => "PRIORITIZE",
            PrimitiveName::Compare => "COMPARE",
            PrimitiveName::Decide => "DECIDE",
            PrimitiveName::Route => "ROUTE",
            PrimitiveName::Schedule => "SCHEDULE",
            PrimitiveName::Execute => "EXECUTE",
            PrimitiveName::Reconcile => "RECONCILE",
            PrimitiveName::Emit => "EMIT",
            PrimitiveName::Persist => "PERSIST",
            PrimitiveName::Confirm => "CONFIRM",
            PrimitiveName::Cancel => "CANCEL",
        }
    }
}

impl std::fmt::Display for PrimitiveName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Extracts the capability-matching kind string when present.
///
/// For `Collect/Fetch/Compress/Classify/Prioritize/Compare` this is the
/// declared `Kind` field. For `Execute` it is the `ActionKind` name (e.g.
/// `"outbound.send"`, `"host.pair"`) — that is the natural semantic
/// discriminator for capability bindings on action primitives.
pub fn primitive_kind(p: &IRPrimitive) -> Option<&str> {
    use crate::ir::ActionKind;
    match p {
        IRPrimitive::Collect { kind, .. }
        | IRPrimitive::Fetch { kind, .. }
        | IRPrimitive::Compress { kind, .. }
        | IRPrimitive::Classify { kind, .. }
        | IRPrimitive::Prioritize { kind, .. }
        | IRPrimitive::Compare { kind, .. } => Some(kind.0.as_str()),
        IRPrimitive::Execute { action, .. } => match action {
            ActionKind::Canonical(id) => Some(id.dotted_str()),
            ActionKind::Custom(n) => Some(n.as_str()),
            ActionKind::HostReconcile => Some("host.reconcile"),
        },
        _ => None,
    }
}

impl CapabilityManifest {
    pub fn can_realize(&self, p: &IRPrimitive) -> bool {
        self.supported_primitives
            .contains(&PrimitiveName::from_primitive(p))
    }

    /// Kind constraint: empty `supported_kinds` means wildcard.
    pub fn kind_allowed(&self, p: &IRPrimitive) -> bool {
        if self.supported_kinds.is_empty() {
            return true;
        }
        match primitive_kind(p) {
            Some(k) => self.supported_kinds.contains(k),
            None => true,
        }
    }

    /// Evidence realizability: if any guarantee is required for execution, substrate must declare `evidence.write`.
    pub fn evidence_realizable(&self, require_evidence: bool) -> bool {
        if !require_evidence {
            return true;
        }
        self.declared_guarantees.contains("evidence.write")
    }

    /// First binding that would cover the given primitive, honoring
    /// `kind_filter` rules. Intended for the planning/routing compiler.
    pub fn binding_for<'a>(&'a self, p: &IRPrimitive) -> Option<&'a CapabilityBinding> {
        let name = PrimitiveName::from_primitive(p);
        let kind = primitive_kind(p);
        self.bindings
            .iter()
            .find(|b| b.primitive == name && b.matches_kind(kind))
    }
}

// ---------------------------------------------------------------------------
// Capability envelopes — Phase 1 of the transplant plan.
//
// Each envelope is a first-class *runtime contract*: it describes what the
// substrate promises for a specific primitive, not what the caller hopes for.
// The planning compiler will pick a substrate by matching the IR node's
// requirements against the envelope shape below.
// ---------------------------------------------------------------------------

/// One capability binding: a primitive (optionally narrowed by kind) mapped to
/// the full set of envelopes that describe what executing through this
/// substrate yields.
///
/// # Identity (ADR 09)
///
/// For `Execute` primitives the binding's authority is its
/// [`CanonicalActionId`], not a free-form `kind_filter` string. The
/// `action_identity` field carries that typed identity; when present, it
/// takes precedence over `kind_filter` during realizability checks. Legacy
/// bindings that only declare `kind_filter` continue to work — the
/// constructor [`CapabilityBinding::for_action`] is the preferred path for
/// new code, and [`CapabilityBinding::action_identity_or_derived`] lets
/// downstream consumers ask "what canonical action does this binding
/// attach to?" without string splitting.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityBinding {
    pub primitive: PrimitiveName,
    /// If `Some`, this binding only applies when the IR primitive's `kind`
    /// matches. If `None`, the binding is a wildcard for that primitive.
    #[serde(default)]
    pub kind_filter: Option<String>,
    /// PR 5b.2 / ADR 09: canonical action identity for `Execute` bindings.
    /// When present, this is the authoritative identity — the
    /// `kind_filter` becomes a derived rendering (the dotted surface form)
    /// and is kept in sync by [`CapabilityBinding::for_action`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_identity: Option<CanonicalActionId>,
    #[serde(default)]
    pub guarantee: GuaranteeEnvelope,
    #[serde(default)]
    pub cost: CostEnvelope,
    #[serde(default)]
    pub latency: LatencyEnvelope,
    #[serde(default)]
    pub evidence: EvidenceGuarantee,
}

impl CapabilityBinding {
    /// Preferred constructor for `Execute` bindings: takes the canonical
    /// identity as input. Sets `primitive = Execute`, attaches the typed
    /// `action_identity`, and keeps `kind_filter` in sync with its
    /// surface form so legacy consumers see the same matching behavior.
    pub fn for_action(action: CanonicalActionId) -> Self {
        Self {
            primitive: PrimitiveName::Execute,
            kind_filter: Some(action.surface_kind()),
            action_identity: Some(action),
            ..Default::default()
        }
    }

    /// Match a kind string against this binding. When `action_identity`
    /// is set it is the authority; otherwise `kind_filter` is consulted
    /// in the legacy way. The two are always equivalent for bindings
    /// built through [`CapabilityBinding::for_action`].
    pub fn matches_kind(&self, kind: Option<&str>) -> bool {
        if let Some(id) = &self.action_identity {
            return match kind {
                Some(k) => k == id.dotted(),
                None => false,
            };
        }
        match (&self.kind_filter, kind) {
            (None, _) => true,
            (Some(filter), Some(k)) => filter == k,
            (Some(_), None) => false,
        }
    }

    /// Return the canonical action identity this binding attaches to,
    /// if any. Returns `Some` when `action_identity` is explicitly set,
    /// or when `primitive == Execute` and `kind_filter` can be parsed as
    /// a [`CanonicalActionId`] (legacy-compatibility projection). Returns
    /// `None` for non-`Execute` primitives or malformed `kind_filter`
    /// strings — callers must handle that explicitly rather than guess.
    pub fn action_identity_or_derived(&self) -> Option<CanonicalActionId> {
        if let Some(id) = &self.action_identity {
            return Some(id.clone());
        }
        if self.primitive == PrimitiveName::Execute {
            if let Some(s) = &self.kind_filter {
                return CanonicalActionId::parse(s).ok();
            }
        }
        None
    }
}

/// What the substrate guarantees about the *semantics* of execution.
///
/// These are constitutional properties (atomicity, ordering, determinism)
/// distinct from performance properties (cost/latency) and auditability
/// (evidence).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GuaranteeEnvelope {
    /// If true, the substrate executes the action as an atomic unit (all or
    /// nothing) with respect to external observers.
    #[serde(default)]
    pub atomic: bool,
    /// If true, two successful executions with identical inputs produce
    /// identical observable outcomes.
    #[serde(default)]
    pub deterministic: bool,
    /// If true, the substrate preserves submission order for a single
    /// correlation/target pair.
    #[serde(default)]
    pub preserves_order: bool,
    /// Free-form guarantee tags beyond the closed booleans above (e.g.
    /// "at_least_once", "exactly_once", "read_your_writes"). Unknown tags
    /// are preserved verbatim; the planner treats them as opaque.
    #[serde(default)]
    pub tags: BTreeSet<String>,
}

/// Cost shape for a single execution on this substrate. All fields are
/// optional — a substrate may decline to disclose any of them and the
/// planner must tolerate absence.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CostEnvelope {
    /// Monetary cost in micro-units of the declared currency.
    #[serde(default)]
    pub monetary_micros: Option<u64>,
    #[serde(default)]
    pub currency: Option<String>,
    /// LLM tokens consumed if the substrate is model-backed.
    #[serde(default)]
    pub tokens: Option<u64>,
    /// Free-form per-unit tags (e.g. "per_send", "per_call"). Opaque to the
    /// planner today; grounds future cost aggregation.
    #[serde(default)]
    pub tags: BTreeSet<String>,
}

/// Latency shape for a single execution. `p50` and `p95` are milliseconds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatencyEnvelope {
    #[serde(default)]
    pub p50_ms: Option<u32>,
    #[serde(default)]
    pub p95_ms: Option<u32>,
    /// Maximum the substrate will wait before returning a definite failure.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// What the substrate promises about evidence emission for this primitive.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceGuarantee {
    /// The substrate writes evidence rows for successful execution.
    #[serde(default)]
    pub writes_success: bool,
    /// The substrate writes evidence rows for failures too (not silent drops).
    #[serde(default)]
    pub writes_failure: bool,
    /// The substrate's writes are durable before returning to the caller.
    #[serde(default)]
    pub durable_before_ack: bool,
    /// Declared evidence kinds this binding is committed to emit on the
    /// happy path. Mirrors the `LoweringPlan.evidence.required_kinds`
    /// vocabulary so the planner can cross-check.
    #[serde(default)]
    pub declared_kinds: Vec<String>,
}

// `Default` for `PrimitiveName` is derived above with `#[default]` on `Observe`.
// `Observe` is the semantically cheapest default, needed for `#[serde(default)]`
// on `CapabilityBinding`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrips_through_json_with_empty_bindings() {
        let m = CapabilityManifest {
            substrate_id: "platform".into(),
            substrate_version: "v1".into(),
            supported_primitives: [PrimitiveName::Execute].into_iter().collect(),
            ..Default::default()
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: CapabilityManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
        assert!(back.bindings.is_empty());
    }

    #[test]
    fn legacy_manifest_json_without_bindings_field_still_deserializes() {
        // Guarantees backward compatibility: older serialized manifests had
        // no `bindings` field; they must still load.
        let s = r#"{
            "substrate_id": "legacy",
            "substrate_version": "v0",
            "supported_primitives": ["EXECUTE"],
            "supported_kinds": [],
            "declared_guarantees": []
        }"#;
        let m: CapabilityManifest = serde_json::from_str(s).unwrap();
        assert!(m.bindings.is_empty());
    }

    #[test]
    fn binding_matches_kind_honors_wildcard_and_filter() {
        let wildcard = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: None,
            ..Default::default()
        };
        let filtered = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("outbound.send".into()),
            ..Default::default()
        };
        assert!(wildcard.matches_kind(None));
        assert!(wildcard.matches_kind(Some("whatever")));
        assert!(filtered.matches_kind(Some("outbound.send")));
        assert!(!filtered.matches_kind(Some("host.pair")));
        assert!(
            !filtered.matches_kind(None),
            "a kind-filtered binding must not match primitives that carry no kind"
        );
    }

    #[test]
    fn binding_for_finds_primitive_and_kind_respecting_filter() {
        use crate::ir::{ActionKind, IRPrimitive};
        let m = CapabilityManifest {
            substrate_id: "provider".into(),
            substrate_version: "v1".into(),
            bindings: vec![
                CapabilityBinding {
                    primitive: PrimitiveName::Execute,
                    kind_filter: Some("outbound.send".into()),
                    evidence: EvidenceGuarantee {
                        writes_success: true,
                        writes_failure: true,
                        durable_before_ack: true,
                        declared_kinds: vec!["outbound.sent".into()],
                    },
                    ..Default::default()
                },
                CapabilityBinding {
                    primitive: PrimitiveName::Execute,
                    kind_filter: Some("host.pair".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let exec_outbound = IRPrimitive::Execute {
            action: ActionKind::Canonical(CanonicalActionId::parse("outbound.send").unwrap()),
            params: Default::default(),
        };
        let b = m.binding_for(&exec_outbound).unwrap();
        assert_eq!(b.kind_filter.as_deref(), Some("outbound.send"));
        assert!(b.evidence.writes_success);

        let exec_unknown = IRPrimitive::Execute {
            action: ActionKind::Canonical(CanonicalActionId::parse("mystery.act").unwrap()),
            params: Default::default(),
        };
        assert!(m.binding_for(&exec_unknown).is_none());
    }

    // -----------------------------------------------------------------
    // PR 5b.2 / ADR 09 — typed action identity on bindings
    // -----------------------------------------------------------------

    #[test]
    fn for_action_constructor_sets_execute_and_mirrors_kind_filter() {
        let id = CanonicalActionId::parse("outbound.send").unwrap();
        let b = CapabilityBinding::for_action(id.clone());
        assert_eq!(b.primitive, PrimitiveName::Execute);
        assert_eq!(b.action_identity.as_ref(), Some(&id));
        // kind_filter kept in sync for legacy matchers.
        assert_eq!(b.kind_filter.as_deref(), Some("outbound.send"));
    }

    #[test]
    fn typed_identity_takes_precedence_over_kind_filter() {
        // Pathological binding: typed identity says host.pair but
        // kind_filter says outbound.send. Authority is the typed form.
        let b = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("outbound.send".into()), // stale rendering
            action_identity: Some(CanonicalActionId::parse("host.pair").unwrap()),
            ..Default::default()
        };
        assert!(b.matches_kind(Some("host.pair")));
        assert!(!b.matches_kind(Some("outbound.send")));
        assert!(!b.matches_kind(None));
    }

    #[test]
    fn legacy_binding_without_typed_identity_still_matches_by_kind_filter() {
        let legacy = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("outbound.send".into()),
            action_identity: None,
            ..Default::default()
        };
        assert!(legacy.matches_kind(Some("outbound.send")));
        assert!(!legacy.matches_kind(Some("host.pair")));
    }

    #[test]
    fn action_identity_or_derived_projects_from_either_source() {
        // Typed source — direct.
        let typed = CapabilityBinding::for_action(CanonicalActionId::parse("host.pair").unwrap());
        assert_eq!(
            typed.action_identity_or_derived().unwrap().dotted(),
            "host.pair"
        );

        // Legacy source — parsed from kind_filter.
        let legacy = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("outbound.send".into()),
            action_identity: None,
            ..Default::default()
        };
        assert_eq!(
            legacy.action_identity_or_derived().unwrap().dotted(),
            "outbound.send"
        );

        // Non-Execute primitive — no action identity derivation.
        let collect = CapabilityBinding {
            primitive: PrimitiveName::Collect,
            kind_filter: Some("events".into()),
            action_identity: None,
            ..Default::default()
        };
        assert!(collect.action_identity_or_derived().is_none());

        // Execute with malformed kind_filter — surfaces None honestly, not
        // a guess. ADR 09 forbids dispatch on ambiguous string reconstruction.
        let malformed = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("not-canonical".into()),
            action_identity: None,
            ..Default::default()
        };
        assert!(malformed.action_identity_or_derived().is_none());
    }

    #[test]
    fn binding_for_still_resolves_via_typed_identity() {
        use crate::ir::{ActionKind, IRPrimitive};
        let m = CapabilityManifest {
            substrate_id: "provider".into(),
            substrate_version: "v1".into(),
            bindings: vec![CapabilityBinding::for_action(
                CanonicalActionId::parse("outbound.send").unwrap(),
            )],
            ..Default::default()
        };
        let node = IRPrimitive::Execute {
            action: ActionKind::Canonical(CanonicalActionId::parse("outbound.send").unwrap()),
            params: Default::default(),
        };
        let b = m.binding_for(&node).expect("typed binding must resolve");
        assert_eq!(
            b.action_identity_or_derived().unwrap().dotted(),
            "outbound.send"
        );
    }

    #[test]
    fn typed_binding_serde_roundtrips_and_omits_field_when_absent() {
        let typed = CapabilityBinding::for_action(CanonicalActionId::parse("host.pair").unwrap());
        let s = serde_json::to_string(&typed).unwrap();
        assert!(s.contains(r#""action_identity""#));
        let back: CapabilityBinding = serde_json::from_str(&s).unwrap();
        assert_eq!(typed, back);

        // Binding without typed identity must omit the field on the wire
        // so legacy consumers' deserializers are untouched.
        let legacy = CapabilityBinding {
            primitive: PrimitiveName::Execute,
            kind_filter: Some("outbound.send".into()),
            ..Default::default()
        };
        let s = serde_json::to_string(&legacy).unwrap();
        assert!(!s.contains(r#""action_identity""#));
    }

    #[test]
    fn legacy_manifest_json_without_action_identity_field_still_deserializes() {
        // A manifest produced before PR 5b.2 had only `kind_filter`. It
        // must still load cleanly and match by legacy path.
        let s = r#"{
            "substrate_id": "legacy",
            "substrate_version": "v0",
            "supported_primitives": ["EXECUTE"],
            "supported_kinds": [],
            "declared_guarantees": [],
            "bindings": [{
                "primitive": "EXECUTE",
                "kind_filter": "outbound.send"
            }]
        }"#;
        let m: CapabilityManifest = serde_json::from_str(s).unwrap();
        assert_eq!(m.bindings.len(), 1);
        assert!(m.bindings[0].action_identity.is_none());
        assert!(m.bindings[0].matches_kind(Some("outbound.send")));
    }

    #[test]
    fn envelope_defaults_are_empty_and_opaque() {
        let b = CapabilityBinding::default();
        assert!(!b.guarantee.atomic);
        assert!(!b.guarantee.deterministic);
        assert!(!b.guarantee.preserves_order);
        assert!(b.guarantee.tags.is_empty());
        assert!(b.cost.monetary_micros.is_none());
        assert!(b.latency.p50_ms.is_none());
        assert!(!b.evidence.writes_success);
        assert!(b.evidence.declared_kinds.is_empty());
    }
}
