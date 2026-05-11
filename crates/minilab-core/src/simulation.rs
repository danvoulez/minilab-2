use serde::{Deserialize, Serialize};

/// Execution substrate mode for a Minilab run.
///
/// This is intentionally distinct from exploration-layer simulation specs.
/// `SimMode` answers "which substrate is this run using?";
/// exploration answers "which elastic branch are we evaluating?".
///
/// Simulation is NOT a separate stack — the same IR, registry, departments,
/// lowerers, policy engine, and KPIs are used in every mode. Only inputs and
/// substrates differ. See `docs/minilab/manifesto.md` for the invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimMode {
    /// Real signals, real prospects, real sends, real replies.
    #[default]
    Production,
    /// Replayed historical signals and evidence — same flow, no external side-effects.
    Replay,
    /// Synthetic inputs, modelled replies — same constitution, alternate substrate.
    Simulation,
    /// Counterfactual branch: ask "what if?" without touching production evidence.
    Counterfactual,
}

impl SimMode {
    pub const fn is_production(self) -> bool {
        matches!(self, SimMode::Production)
    }

    /// True for any mode that must NOT emit real outbound messages.
    pub const fn blocks_real_sends(self) -> bool {
        !matches!(self, SimMode::Production)
    }
}
