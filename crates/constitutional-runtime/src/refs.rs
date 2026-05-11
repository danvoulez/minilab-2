//! Canonical reference types for IR and lowering.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable node identifier in an intent graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Opaque handle to persisted or in-flight data.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct DataRef(pub String);

/// Host, service, or logical target.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct TargetRef(pub String);

/// UI or integration surface (e.g. thread, channel).
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRef(pub String);

/// Named policy bundle for admission.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct PolicyId(pub String);
