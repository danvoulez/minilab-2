use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SimBranch {
    pub mode: SimMode,
    pub branch_id: String,
    pub parent_branch_id: Option<String>,
}

impl SimBranch {
    pub fn production() -> Self {
        Self {
            mode: SimMode::Production,
            branch_id: "production".into(),
            parent_branch_id: None,
        }
    }

    pub fn for_mode(mode: SimMode) -> Self {
        match mode {
            SimMode::Production => Self::production(),
            _ => Self::fork(mode, "production"),
        }
    }

    pub fn fork(mode: SimMode, parent_branch_id: impl Into<String>) -> Self {
        let parent_branch_id = parent_branch_id.into();
        Self {
            mode,
            branch_id: format!("{}:{}", mode.as_str(), Uuid::new_v4()),
            parent_branch_id: Some(parent_branch_id),
        }
    }

    pub fn from_parts(
        mode: SimMode,
        branch_id: impl Into<String>,
        parent_branch_id: Option<String>,
    ) -> Self {
        Self {
            mode,
            branch_id: branch_id.into(),
            parent_branch_id,
        }
    }
}

impl SimMode {
    pub const fn is_production(self) -> bool {
        matches!(self, SimMode::Production)
    }

    /// True for any mode that must NOT emit real outbound messages.
    pub const fn blocks_real_sends(self) -> bool {
        !matches!(self, SimMode::Production)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            SimMode::Production => "production",
            SimMode::Replay => "replay",
            SimMode::Simulation => "simulation",
            SimMode::Counterfactual => "counterfactual",
        }
    }
}
