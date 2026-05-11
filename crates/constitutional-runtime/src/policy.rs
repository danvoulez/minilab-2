//! Policy classes: regimes of admissibility (A/B/C/D).
//!
//! These are **constitutional** labels, not informal severity. They gate what may cross
//! the runtime execution boundary together with [`crate::validation::AdmissibilityContext`].

use serde::{Deserialize, Serialize};

/// Constitutional regime for what may cross the lowering / execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyClass {
    /// **Autonomous safe:** read-only intelligence loop plus bounded emit / route / schedule.
    /// No direct material mutation, persistence, confirmation checkpoints, or reconcile.
    A,
    /// **Bounded material:** same as A for risk profile, plus execution and related effects,
    /// except [`crate::ir::IRPrimitive::Reconcile`] (host-level reconcile stays out of this band).
    B,
    /// **Authority-sensitive:** full primitive set at the runtime boundary when capability matches;
    /// confirmation and authority are expected to align with lowering output (e.g. `Confirm`, risky `Execute`).
    C,
    /// **Reserved band:** IR may **represent** these nodes in a planning or audit graph, but they are
    /// **not executable at the normal runtime boundary**. Use a deliberation layer, human authority path,
    /// or explicit exception — not `validate_admissibility` with `at_execution_boundary: true`.
    D,
}
