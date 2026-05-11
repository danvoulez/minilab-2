use serde::{Deserialize, Serialize};

/// Jurisdictions in the mini-company spine (see `docs/minilab/manifesto.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Department {
    Research,
    SalesOps,
    Marketing,
    Sdr,
    Compliance,
    MemoryAnalytics,
    /// Strategic / constitutional acts only; no routine autonomy.
    Founder,
    /// Distributed place-local agent (post-v1 horizon).
    PlaceAgent,
}

impl Department {
    pub const fn as_str(self) -> &'static str {
        match self {
            Department::Research => "research",
            Department::SalesOps => "sales_ops",
            Department::Marketing => "marketing",
            Department::Sdr => "sdr",
            Department::Compliance => "compliance",
            Department::MemoryAnalytics => "memory_analytics",
            Department::Founder => "founder",
            Department::PlaceAgent => "place_agent",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Department::Research => "Research",
            Department::SalesOps => "Sales Ops / prospect graph",
            Department::Marketing => "Marketing",
            Department::Sdr => "SDR / outreach",
            Department::Compliance => "Compliance / constitutional",
            Department::MemoryAnalytics => "Memory / analytics",
            Department::Founder => "Founder",
            Department::PlaceAgent => "Place Agent",
        }
    }
}

/// How much autonomous latitude a department run has.
///
/// Separate from `PolicyClass` (which lives in `constitutional-runtime`).
/// `AutonomyClass` encodes *operating permissions* of a department;
/// `PolicyClass` encodes *semantic risk* of a single node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyClass {
    /// Human / operator action only — no autonomous execution.
    Manual,
    /// Runs within narrow pre-approved templates.
    BoundedAutonomous,
    /// Loops continuously inside a declared jurisdiction.
    HighAutonomy,
    /// Only founder / compliance constitutional acts permitted.
    ConstitutionalOnly,
}

impl std::fmt::Display for Department {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
