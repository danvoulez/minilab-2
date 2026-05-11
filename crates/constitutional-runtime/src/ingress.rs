//! Ingress escalation: cheap paths first; premium LLM is constitutional exception only.
//!
//! This module is intentionally small in the Phase A baseline. The vocabulary is
//! public because docs and downstream callers name the L0-L3 ingress ladder, but
//! the backend that consumes it is scheduled for the Agent Runtime phase.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum IngressMode {
    /// Rules, templates, cached mappings.
    FastPath,
    /// Small model, constrained decoding to structured intent.
    Structured,
    /// Full Place Agent composition.
    Cognitive,
    /// Rare: appellate-style resolution when lower tiers cannot close.
    PremiumException,
}

impl IngressMode {
    /// Stable ladder rank used for ordering ingress modes from cheapest to most exceptional.
    pub const fn rank(self) -> u8 {
        match self {
            Self::FastPath => 0,
            Self::Structured => 1,
            Self::Cognitive => 2,
            Self::PremiumException => 3,
        }
    }

    /// Whether this mode is the appellate-style premium exception lane.
    pub const fn is_premium_exception(self) -> bool {
        matches!(self, Self::PremiumException)
    }
}

#[cfg(test)]
mod tests {
    use super::IngressMode;

    #[test]
    fn ingress_mode_serializes_with_documented_pascal_case_names() {
        let s = serde_json::to_string(&IngressMode::PremiumException).unwrap();
        assert_eq!(s, "\"PremiumException\"");

        let back: IngressMode = serde_json::from_str("\"FastPath\"").unwrap();
        assert_eq!(back, IngressMode::FastPath);
    }

    #[test]
    fn ingress_mode_rank_orders_ladder_from_fast_to_exceptional() {
        assert!(IngressMode::FastPath.rank() < IngressMode::Structured.rank());
        assert!(IngressMode::Structured.rank() < IngressMode::Cognitive.rank());
        assert!(IngressMode::Cognitive.rank() < IngressMode::PremiumException.rank());
        assert!(IngressMode::PremiumException.is_premium_exception());
    }
}
