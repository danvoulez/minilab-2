//! Canonical act identity — the authoritative identity of a material act
//! as frozen by `extracted/key/09-act-identity-authority.md`.
//!
//! # Decision
//!
//! The authoritative identity of a material act is its **Canonical IR
//! Action Identity**. Operational surface kinds and lowered runtime
//! command triples are **derived renderings** and must not be treated as
//! independent semantic authorities.
//!
//! # What this module gives you
//!
//! - [`CanonicalActionId`] — the typed, governed identity.
//! - [`CanonicalActionId::surface_kind`] — projection to the operational
//!   grammar surface form.
//! - [`CanonicalActionId::lowered_command_identity`] — projection to the
//!   `(namespace, verb)` pair used by [`crate::lowering::OperationalCommand`].
//! - [`CanonicalActionId::from_action_kind`] /
//!   [`CanonicalActionId::to_action_kind`] — conversion to and from the
//!   current IR carrier [`crate::ir::ActionKind::Canonical`].
//! - [`IdentityError`] — closed error set for malformed identities.
//!
//! # What this module does **not** do
//!
//! - It does **not** change every legacy caller to construct canonical acts
//!   up front; adapters may still parse dotted strings at their boundaries.
//! - It does **not** erase [`crate::ir::ActionKind::Custom`] or
//!   [`crate::ir::ActionKind::HostReconcile`]; those remain explicit
//!   non-canonical carriers.
//! - It does **not** touch executor policy. This module only governs act
//!   identity and its projections.
//!
//! # Construction rules
//!
//! A canonical identity is a pair `(namespace, verb)` where each side is:
//!
//! - non-empty,
//! - composed only of ASCII lowercase letters, digits, and `_`,
//! - and separated by exactly one `.` in the dotted form.
//!
//! These rules exist so that identity is unambiguous by construction and
//! never needs to be rebuilt from arbitrary caller strings downstream.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::ir::ActionKind;

/// The canonical identity of a material act at the IR layer.
///
/// Carries `namespace` and `verb` plus a precomputed dotted form. The
/// dotted form is immutable and equal to `format!("{namespace}.{verb}")`
/// — built once at construction so borrowed accessors don't allocate on
/// hot paths (e.g. the IR primitive kind dispatch). Serialization uses a
/// two-field representation (`{namespace, verb}`); the `dotted` cache is
/// recomputed on deserialize, so wire-format stays stable and compact.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "CanonicalActionIdRepr", into = "CanonicalActionIdRepr")]
pub struct CanonicalActionId {
    namespace: String,
    verb: String,
    dotted: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CanonicalActionIdRepr {
    namespace: String,
    verb: String,
}

impl TryFrom<CanonicalActionIdRepr> for CanonicalActionId {
    type Error = IdentityError;

    fn try_from(r: CanonicalActionIdRepr) -> Result<Self, Self::Error> {
        CanonicalActionId::new(r.namespace, r.verb)
    }
}

impl From<CanonicalActionId> for CanonicalActionIdRepr {
    fn from(id: CanonicalActionId) -> Self {
        Self {
            namespace: id.namespace,
            verb: id.verb,
        }
    }
}

/// Closed error set for identity acquisition. No free strings from the
/// constitutional boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityError {
    /// The dotted form had the wrong shape: zero or multiple separators,
    /// empty namespace, empty verb, or trailing/leading whitespace.
    Malformed { input: String, reason: &'static str },
    /// A component contained a character outside the allowed set.
    InvalidCharacter {
        input: String,
        component: &'static str,
    },
    /// The carrier [`ActionKind`] does not encode a canonical identity.
    NotNamed,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityError::Malformed { input, reason } => {
                write!(f, "malformed canonical action id {input:?}: {reason}")
            }
            IdentityError::InvalidCharacter { input, component } => write!(
                f,
                "invalid character in {component} of canonical action id {input:?}"
            ),
            IdentityError::NotNamed => {
                f.write_str("ActionKind does not carry a canonical identity")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

impl CanonicalActionId {
    /// Build from already-validated components. Returns an error if either
    /// side violates the construction rules.
    pub fn new(
        namespace: impl Into<String>,
        verb: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        let namespace = namespace.into();
        let verb = verb.into();
        validate_component(&namespace, "namespace")?;
        validate_component(&verb, "verb")?;
        let dotted = format!("{namespace}.{verb}");
        Ok(Self {
            namespace,
            verb,
            dotted,
        })
    }

    /// Parse from the dotted form (`"namespace.verb"`).
    pub fn parse(dotted: &str) -> Result<Self, IdentityError> {
        if dotted.chars().any(char::is_whitespace) {
            return Err(IdentityError::Malformed {
                input: dotted.into(),
                reason: "whitespace not allowed",
            });
        }
        let dot_count = dotted.matches('.').count();
        if dot_count == 0 {
            return Err(IdentityError::Malformed {
                input: dotted.into(),
                reason: "missing '.' separator",
            });
        }
        if dot_count > 1 {
            return Err(IdentityError::Malformed {
                input: dotted.into(),
                reason: "exactly one '.' separator required",
            });
        }
        let (ns, verb) = dotted.split_once('.').unwrap();
        if ns.is_empty() {
            return Err(IdentityError::Malformed {
                input: dotted.into(),
                reason: "empty namespace",
            });
        }
        if verb.is_empty() {
            return Err(IdentityError::Malformed {
                input: dotted.into(),
                reason: "empty verb",
            });
        }
        Self::new(ns.to_string(), verb.to_string())
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// Canonical dotted form (`"outbound.send"`) as an owned string.
    /// Alias for [`fmt::Display`].
    pub fn dotted(&self) -> String {
        self.dotted.clone()
    }

    /// Borrow of the precomputed dotted form. Zero-alloc. Prefer this on
    /// hot paths (e.g. IR primitive kind dispatch, matcher keys).
    pub fn dotted_str(&self) -> &str {
        &self.dotted
    }

    // ---------------- Projections (ADR 09 §"Projeções permitidas") ----------------

    /// Projection to the operational grammar surface form. Explicit
    /// derivation — not aliasing.
    pub fn surface_kind(&self) -> String {
        self.dotted()
    }

    /// Projection to the `(namespace, verb)` pair that
    /// [`crate::lowering::OperationalCommand`] exposes. Explicit derivation
    /// — callers must not reconstruct this tuple by string splitting.
    pub fn lowered_command_identity(&self) -> (&str, &str) {
        (&self.namespace, &self.verb)
    }

    // ---------------- Conversions to/from the current IR carrier -----------------

    /// Extract a canonical identity from an [`ActionKind`]. Returns
    /// `Ok` for [`ActionKind::Canonical`] (zero cost — the identity is
    /// already typed). Fails for `Custom` and `HostReconcile`.
    pub fn from_action_kind(action: &ActionKind) -> Result<Self, IdentityError> {
        match action {
            ActionKind::Canonical(id) => Ok(id.clone()),
            ActionKind::Custom(_) | ActionKind::HostReconcile => Err(IdentityError::NotNamed),
        }
    }

    /// Render this identity into the IR carrier. PR 5b.1: this is a
    /// zero-cost type wrap around the typed identity.
    pub fn to_action_kind(&self) -> ActionKind {
        ActionKind::Canonical(self.clone())
    }
}

impl fmt::Display for CanonicalActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace, self.verb)
    }
}

fn validate_component(s: &str, component: &'static str) -> Result<(), IdentityError> {
    if s.is_empty() {
        return Err(IdentityError::Malformed {
            input: s.into(),
            reason: match component {
                "namespace" => "empty namespace",
                "verb" => "empty verb",
                _ => "empty component",
            },
        });
    }
    for c in s.chars() {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
        if !ok {
            return Err(IdentityError::InvalidCharacter {
                input: s.into(),
                component,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrips_dotted_form() {
        let id = CanonicalActionId::parse("outbound.send").unwrap();
        assert_eq!(id.namespace(), "outbound");
        assert_eq!(id.verb(), "send");
        assert_eq!(id.dotted(), "outbound.send");
        assert_eq!(id.to_string(), "outbound.send");
    }

    #[test]
    fn parse_rejects_missing_dot() {
        let err = CanonicalActionId::parse("outboundsend").unwrap_err();
        assert!(matches!(
            err,
            IdentityError::Malformed {
                reason: "missing '.' separator",
                ..
            }
        ));
    }

    #[test]
    fn parse_rejects_multiple_dots() {
        let err = CanonicalActionId::parse("outbound.send.retry").unwrap_err();
        assert!(matches!(
            err,
            IdentityError::Malformed {
                reason: "exactly one '.' separator required",
                ..
            }
        ));
    }

    #[test]
    fn parse_rejects_empty_parts() {
        assert!(matches!(
            CanonicalActionId::parse(".send").unwrap_err(),
            IdentityError::Malformed {
                reason: "empty namespace",
                ..
            }
        ));
        assert!(matches!(
            CanonicalActionId::parse("outbound.").unwrap_err(),
            IdentityError::Malformed {
                reason: "empty verb",
                ..
            }
        ));
    }

    #[test]
    fn parse_rejects_whitespace_and_uppercase_and_punctuation() {
        assert!(matches!(
            CanonicalActionId::parse(" outbound.send").unwrap_err(),
            IdentityError::Malformed {
                reason: "whitespace not allowed",
                ..
            }
        ));
        assert!(matches!(
            CanonicalActionId::parse("Outbound.send").unwrap_err(),
            IdentityError::InvalidCharacter {
                component: "namespace",
                ..
            }
        ));
        assert!(matches!(
            CanonicalActionId::parse("outbound.send!").unwrap_err(),
            IdentityError::InvalidCharacter {
                component: "verb",
                ..
            }
        ));
    }

    #[test]
    fn allows_lowercase_digits_underscore() {
        let id = CanonicalActionId::parse("host_v2.pair_check3").unwrap();
        assert_eq!(id.namespace(), "host_v2");
        assert_eq!(id.verb(), "pair_check3");
    }

    #[test]
    fn surface_kind_projection_is_the_dotted_form() {
        let id = CanonicalActionId::new("outbound", "send").unwrap();
        assert_eq!(id.surface_kind(), "outbound.send");
    }

    #[test]
    fn lowered_command_identity_projection_is_the_pair() {
        let id = CanonicalActionId::new("host", "pair").unwrap();
        assert_eq!(id.lowered_command_identity(), ("host", "pair"));
    }

    #[test]
    fn from_action_kind_canonical_roundtrips_without_reparse() {
        let id = CanonicalActionId::parse("outbound.send").unwrap();
        let a = ActionKind::Canonical(id.clone());
        let back = CanonicalActionId::from_action_kind(&a).unwrap();
        assert_eq!(back, id);
        assert_eq!(back.to_action_kind(), a);
    }

    #[test]
    fn from_action_kind_rejects_custom_and_host_reconcile() {
        let custom = ActionKind::Custom("opaque.command".into());
        assert!(matches!(
            CanonicalActionId::from_action_kind(&custom),
            Err(IdentityError::NotNamed)
        ));
        assert!(matches!(
            CanonicalActionId::from_action_kind(&ActionKind::HostReconcile),
            Err(IdentityError::NotNamed)
        ));
    }

    #[test]
    fn equality_and_hash_are_component_driven() {
        use std::collections::HashSet;
        let a = CanonicalActionId::new("outbound", "send").unwrap();
        let b = CanonicalActionId::parse("outbound.send").unwrap();
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn serde_roundtrips_as_struct() {
        let id = CanonicalActionId::new("outbound", "send").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: CanonicalActionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
