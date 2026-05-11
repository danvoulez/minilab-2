//! Ingress escalation: cheap paths first; premium LLM is constitutional exception only.

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
