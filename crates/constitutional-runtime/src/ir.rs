//! Canonical semantic IR: sixteen primitives as the constitution of legitimate acts.

use crate::refs::{DataRef, PolicyId, SurfaceRef, TargetRef};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Surface hint for inference or routing (e.g. which model tier).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferSurface {
    Local,
    Lab256,
    Lab8gb,
    Lab512,
    Cloud,
    Hybrid,
}

impl InferSurface {
    /// Canonical substrate id when this surface names a concrete inference
    /// substrate. `local` / `cloud` / `hybrid` are routing classes, not
    /// substrate ids, so they return `None`.
    pub fn substrate_id(&self) -> Option<&'static str> {
        match self {
            InferSurface::Lab256 => Some("lab256"),
            InferSurface::Lab8gb => Some("lab8gb"),
            InferSurface::Lab512 => Some("lab512"),
            InferSurface::Local | InferSurface::Cloud | InferSurface::Hybrid => None,
        }
    }

    /// The legacy `local` value means "no explicit named inference surface
    /// was selected". The planner may still route such nodes to a concrete
    /// substrate like `lab512`.
    pub fn is_unspecified_default(&self) -> bool {
        matches!(self, InferSurface::Local)
    }
}

/// Time window for collection or comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Window(pub String);

/// Kind discriminator for family of objects (events, hosts, releases, …).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Kind(pub String);

/// Schema identifier for classification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Schema(pub String);

/// Reconciliation mode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileMode {
    Apply,
    DryRun,
    Force,
}

/// Durability expectation for persistence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurabilityClass {
    Ephemeral,
    Durable,
    Audited,
}

/// Role required for confirmation (human role name or policy role).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Role(pub String);

/// What to execute: named operational action vs opaque command envelope.
///
/// # Identity (ADR 09)
///
/// `Canonical` is the primary carrier — a typed
/// [`crate::act_identity::CanonicalActionId`] that the ADR names as the
/// authoritative identity for material acts. `Custom` remains for
/// substrate-specific opaque command envelopes (no canonical identity).
/// `HostReconcile` is kept as an explicit special form for back-compat.
///
/// PR 5b.1 replaced the legacy `Named(String)` carrier with `Canonical`:
/// identity is typed at construction, so no downstream consumer has to
/// parse strings to learn the canonical identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Canonical(crate::act_identity::CanonicalActionId),
    HostReconcile,
    Custom(String),
}

/// Schedule trigger (cron-like or event name).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Trigger(pub String);

/// The sixteen IR primitives.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "primitive", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IRPrimitive {
    Observe {
        target: TargetRef,
        scope: String,
    },
    Collect {
        kind: Kind,
        target: TargetRef,
        window: Window,
    },
    Fetch {
        kind: Kind,
        id: String,
    },
    Compress {
        kind: Kind,
        input_ref: DataRef,
        infer_surface: InferSurface,
    },
    Classify {
        kind: Kind,
        input_ref: DataRef,
        schema: Schema,
        infer_surface: InferSurface,
    },
    Prioritize {
        kind: Kind,
        input_ref: DataRef,
        policy: PolicyId,
        infer_surface: InferSurface,
    },
    Compare {
        kind: Kind,
        left: DataRef,
        right: DataRef,
    },
    Decide {
        context: DataRef,
        policy: PolicyId,
    },
    Route {
        operation: Box<IRPrimitive>,
        surface: SurfaceRef,
    },
    Schedule {
        action: Box<IRPrimitive>,
        trigger: Trigger,
    },
    Execute {
        action: ActionKind,
        params: Map<String, Value>,
    },
    Reconcile {
        target: TargetRef,
        desired: DataRef,
        mode: ReconcileMode,
    },
    Emit {
        surface: SurfaceRef,
        payload: DataRef,
    },
    Persist {
        data: DataRef,
        durability: DurabilityClass,
    },
    Confirm {
        action: Box<IRPrimitive>,
        role: Role,
    },
    Cancel {
        id: String,
    },
}

/// A node in an intent graph (for lowering and audit).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IrNode {
    pub id: crate::refs::NodeId,
    pub body: IRPrimitive,
}
