//! Pure constitutional admission evaluation for proposed LogLine acts.
//!
//! This module is intentionally small and side-effect free. It does not call
//! providers, write evidence, dispatch commands, or read external state; it
//! only classifies a proposed act against the admission context supplied by the
//! caller.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const CLOSED_STATUS: &str = "closed";

/// Proposed nine-slot LogLine act presented to the admission layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposedLogLineAct {
    pub who: String,
    pub did: String,
    pub this: Value,
    pub confirmed_by: Value,
    pub if_ok: String,
    pub if_doubt: String,
    pub if_not: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Closed identity document for an actor that can propose acts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Passport {
    pub passport_id: String,
    pub subject: String,
    pub subject_type: String,
    pub status: String,
}

/// Closed permission envelope constraining which `did` values a holder may use.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Visa {
    pub visa_id: String,
    pub holder: String,
    #[serde(default)]
    pub allowed_dids: Vec<String>,
    #[serde(default)]
    pub forbidden_dids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    pub status: String,
}

/// Closed gate that must exist for an act's `did` before admission can pass.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub gate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_to_dids: Option<Vec<String>>,
    pub fail_closed: bool,
    pub status: String,
}

/// Boundary evaluation class.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    Deterministic,
    Semantic,
}

/// Boundary rule payload for the MVP deterministic rules.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BoundaryRule {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Closed boundary checked during admission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Boundary {
    pub boundary_id: String,
    pub kind: BoundaryKind,
    pub rule: BoundaryRule,
    pub status: String,
}

/// All state needed by pure admission evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissionContext {
    #[serde(default)]
    pub passports: Vec<Passport>,
    #[serde(default)]
    pub visas: Vec<Visa>,
    #[serde(default)]
    pub gates: Vec<Gate>,
    #[serde(default)]
    pub boundaries: Vec<Boundary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
}

/// Final admission class for a proposed act.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionDecision {
    Yes,
    No,
    Cloud,
}

/// Auditable result of pure admission evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissionRuling {
    pub decision: AdmissionDecision,
    pub proposed_act_hash: String,
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passport_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visa_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_id: Option<String>,
    pub boundaries_checked: usize,
    pub boundaries_passed: usize,
    pub boundaries_failed: usize,
    pub boundaries_clouded: usize,
    pub touches_world: bool,
}

/// Evaluate whether a proposed LogLine act is admissible.
///
/// This function is deliberately pure: every input is passed by reference, all
/// output is returned in [`AdmissionRuling`], and no external state is touched.
pub fn evaluate_admission(
    proposed: &ProposedLogLineAct,
    ctx: &AdmissionContext,
) -> AdmissionRuling {
    let mut ruling = AdmissionRuling::new(proposed);

    if let Some(reason) = missing_required_slot(proposed) {
        ruling.no(reason);
        return ruling;
    }

    let Some(passport) = ctx
        .passports
        .iter()
        .find(|passport| passport.subject == proposed.who && is_closed(&passport.status))
    else {
        ruling.no("actor has no closed passport".into());
        return ruling;
    };
    ruling.passport_id = Some(passport.passport_id.clone());

    let Some(visa) = ctx.visas.iter().find(|visa| {
        visa.holder == proposed.who
            && is_closed(&visa.status)
            && visa_time_valid(visa, ctx.now.as_deref())
    }) else {
        ruling.no("actor has no closed valid visa".into());
        return ruling;
    };
    ruling.visa_id = Some(visa.visa_id.clone());

    if visa.forbidden_dids.iter().any(|did| did == &proposed.did) {
        ruling.no(format!("did `{}` is forbidden by visa", proposed.did));
        return ruling;
    }

    if !visa.allowed_dids.iter().any(|did| did == &proposed.did) {
        ruling.no(format!("did `{}` is not allowed by visa", proposed.did));
        return ruling;
    }

    let Some(gate) = ctx.gates.iter().find(|gate| {
        is_closed(&gate.status)
            && gate
                .applies_to_dids
                .as_ref()
                .is_none_or(|dids| dids.iter().any(|did| did == &proposed.did))
    }) else {
        ruling.no("no closed applicable gate".into());
        return ruling;
    };
    ruling.gate_id = Some(gate.gate_id.clone());

    if gate.fail_closed && (gate.gate_id.trim().is_empty() || !is_closed(&gate.status)) {
        ruling.no("fail-closed gate is missing essential data".into());
        return ruling;
    }

    for boundary in ctx
        .boundaries
        .iter()
        .filter(|boundary| is_closed(&boundary.status))
    {
        ruling.boundaries_checked += 1;
        match boundary.kind {
            BoundaryKind::Semantic => {
                ruling.boundaries_clouded += 1;
                ruling.cloud(format!(
                    "semantic boundary `{}` requires CLOUD evaluation",
                    boundary.boundary_id
                ));
                return ruling;
            }
            BoundaryKind::Deterministic => {
                match evaluate_deterministic_boundary(boundary, proposed, visa) {
                    BoundaryEvaluation::Passed => ruling.boundaries_passed += 1,
                    BoundaryEvaluation::Failed(reason) => {
                        ruling.boundaries_failed += 1;
                        ruling.no(reason);
                        return ruling;
                    }
                }
            }
        }
    }

    ruling.yes("admission passed".into());
    ruling
}

impl AdmissionRuling {
    fn new(proposed: &ProposedLogLineAct) -> Self {
        Self {
            decision: AdmissionDecision::No,
            proposed_act_hash: proposed_act_hash(proposed),
            reasons: Vec::new(),
            passport_id: None,
            visa_id: None,
            gate_id: None,
            boundaries_checked: 0,
            boundaries_passed: 0,
            boundaries_failed: 0,
            boundaries_clouded: 0,
            touches_world: false,
        }
    }

    fn yes(&mut self, reason: String) {
        self.decision = AdmissionDecision::Yes;
        self.reasons.push(reason);
    }

    fn no(&mut self, reason: String) {
        self.decision = AdmissionDecision::No;
        self.reasons.push(reason);
    }

    fn cloud(&mut self, reason: String) {
        self.decision = AdmissionDecision::Cloud;
        self.reasons.push(reason);
    }
}

enum BoundaryEvaluation {
    Passed,
    Failed(String),
}

fn evaluate_deterministic_boundary(
    boundary: &Boundary,
    proposed: &ProposedLogLineAct,
    visa: &Visa,
) -> BoundaryEvaluation {
    match boundary.rule.name.as_str() {
        "only_holder_may_do" => match boundary.rule.value.as_deref() {
            Some(value) if proposed.who == value => BoundaryEvaluation::Passed,
            Some(value) => BoundaryEvaluation::Failed(format!(
                "boundary `{}` requires who `{value}`",
                boundary.boundary_id
            )),
            None => BoundaryEvaluation::Failed(format!(
                "boundary `{}` missing value for only_holder_may_do",
                boundary.boundary_id
            )),
        },
        "did_must_not_be" => match boundary.rule.value.as_deref() {
            Some(value) if proposed.did == value => BoundaryEvaluation::Failed(format!(
                "boundary `{}` rejects did `{value}`",
                boundary.boundary_id
            )),
            Some(_) => BoundaryEvaluation::Passed,
            None => BoundaryEvaluation::Failed(format!(
                "boundary `{}` missing value for did_must_not_be",
                boundary.boundary_id
            )),
        },
        "did_must_be_allowed_by_visa" => {
            if visa.allowed_dids.iter().any(|did| did == &proposed.did) {
                BoundaryEvaluation::Passed
            } else {
                BoundaryEvaluation::Failed(format!(
                    "boundary `{}` requires did allowed by visa",
                    boundary.boundary_id
                ))
            }
        }
        other => BoundaryEvaluation::Failed(format!(
            "boundary `{}` has unsupported deterministic rule `{other}`",
            boundary.boundary_id
        )),
    }
}

fn missing_required_slot(proposed: &ProposedLogLineAct) -> Option<String> {
    for (slot, value) in [
        ("who", proposed.who.as_str()),
        ("did", proposed.did.as_str()),
        ("if_ok", proposed.if_ok.as_str()),
        ("if_doubt", proposed.if_doubt.as_str()),
        ("if_not", proposed.if_not.as_str()),
        ("status", proposed.status.as_str()),
    ] {
        if value.trim().is_empty() {
            return Some(format!("required slot `{slot}` is missing"));
        }
    }

    if value_missing(&proposed.this) {
        return Some("required slot `this` is missing".into());
    }
    if value_missing(&proposed.confirmed_by) {
        return Some("required slot `confirmed_by` is missing".into());
    }

    None
}

fn value_missing(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(values) => values.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

fn is_closed(status: &str) -> bool {
    status.eq_ignore_ascii_case(CLOSED_STATUS)
}

fn visa_time_valid(visa: &Visa, now: Option<&str>) -> bool {
    let Some(now) = now else {
        return true;
    };
    if visa.valid_from.as_deref().is_some_and(|from| now < from) {
        return false;
    }
    if visa.valid_until.as_deref().is_some_and(|until| now > until) {
        return false;
    }
    true
}

fn proposed_act_hash(proposed: &ProposedLogLineAct) -> String {
    let bytes =
        serde_json::to_vec(proposed).expect("ProposedLogLineAct serialization is infallible");
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_act() -> ProposedLogLineAct {
        ProposedLogLineAct {
            who: "agent:ana".into(),
            did: "host.inspect".into(),
            this: json!({"target":"lab512"}),
            confirmed_by: json!(["passport:ana"]),
            if_ok: "continue".into(),
            if_doubt: "send to doubt".into(),
            if_not: "reject".into(),
            status: "candidate".into(),
            metadata: None,
        }
    }

    fn passport() -> Passport {
        Passport {
            passport_id: "passport:ana".into(),
            subject: "agent:ana".into(),
            subject_type: "agent".into(),
            status: "closed".into(),
        }
    }

    fn visa() -> Visa {
        Visa {
            visa_id: "visa:ana".into(),
            holder: "agent:ana".into(),
            allowed_dids: vec!["host.inspect".into()],
            forbidden_dids: vec![],
            valid_from: None,
            valid_until: None,
            status: "closed".into(),
        }
    }

    fn gate() -> Gate {
        Gate {
            gate_id: "gate:host".into(),
            applies_to_dids: Some(vec!["host.inspect".into()]),
            fail_closed: true,
            status: "closed".into(),
        }
    }

    fn valid_context() -> AdmissionContext {
        AdmissionContext {
            passports: vec![passport()],
            visas: vec![visa()],
            gates: vec![gate()],
            boundaries: vec![Boundary {
                boundary_id: "boundary:holder".into(),
                kind: BoundaryKind::Deterministic,
                rule: BoundaryRule {
                    name: "only_holder_may_do".into(),
                    value: Some("agent:ana".into()),
                },
                status: "closed".into(),
            }],
            now: None,
        }
    }

    #[test]
    fn admits_valid_act_with_passport_visa_gate() {
        let ruling = evaluate_admission(&valid_act(), &valid_context());

        assert_eq!(ruling.decision, AdmissionDecision::Yes);
        assert_eq!(ruling.passport_id, Some("passport:ana".into()));
        assert_eq!(ruling.visa_id, Some("visa:ana".into()));
        assert_eq!(ruling.gate_id, Some("gate:host".into()));
        assert_eq!(ruling.boundaries_checked, 1);
        assert_eq!(ruling.boundaries_passed, 1);
    }

    #[test]
    fn rejects_missing_passport() {
        let mut ctx = valid_context();
        ctx.passports.clear();

        let ruling = evaluate_admission(&valid_act(), &ctx);

        assert_eq!(ruling.decision, AdmissionDecision::No);
        assert!(ruling
            .reasons
            .iter()
            .any(|reason| reason.contains("passport")));
    }

    #[test]
    fn rejects_missing_visa() {
        let mut ctx = valid_context();
        ctx.visas.clear();

        let ruling = evaluate_admission(&valid_act(), &ctx);

        assert_eq!(ruling.decision, AdmissionDecision::No);
        assert!(ruling.reasons.iter().any(|reason| reason.contains("visa")));
    }

    #[test]
    fn rejects_forbidden_did() {
        let mut ctx = valid_context();
        ctx.visas[0].forbidden_dids = vec!["host.inspect".into()];

        let ruling = evaluate_admission(&valid_act(), &ctx);

        assert_eq!(ruling.decision, AdmissionDecision::No);
        assert!(ruling
            .reasons
            .iter()
            .any(|reason| reason.contains("forbidden")));
    }

    #[test]
    fn rejects_missing_gate() {
        let mut ctx = valid_context();
        ctx.gates.clear();

        let ruling = evaluate_admission(&valid_act(), &ctx);

        assert_eq!(ruling.decision, AdmissionDecision::No);
        assert!(ruling.reasons.iter().any(|reason| reason.contains("gate")));
    }

    #[test]
    fn clouds_semantic_boundary() {
        let mut ctx = valid_context();
        ctx.boundaries = vec![Boundary {
            boundary_id: "boundary:meaning".into(),
            kind: BoundaryKind::Semantic,
            rule: BoundaryRule {
                name: "semantic_review".into(),
                value: None,
            },
            status: "closed".into(),
        }];

        let ruling = evaluate_admission(&valid_act(), &ctx);

        assert_eq!(ruling.decision, AdmissionDecision::Cloud);
        assert_eq!(ruling.boundaries_checked, 1);
        assert_eq!(ruling.boundaries_clouded, 1);
    }

    #[test]
    fn rejects_missing_required_slot() {
        let mut act = valid_act();
        act.did = " ".into();

        let ruling = evaluate_admission(&act, &valid_context());

        assert_eq!(ruling.decision, AdmissionDecision::No);
        assert!(ruling.reasons.iter().any(|reason| reason.contains("did")));
    }

    #[test]
    fn ruling_is_touchless_and_has_hash() {
        let ruling = evaluate_admission(&valid_act(), &valid_context());

        assert!(!ruling.touches_world);
        assert!(ruling.proposed_act_hash.starts_with("sha256:"));
        assert_eq!(ruling.proposed_act_hash.len(), "sha256:".len() + 64);
    }
}
