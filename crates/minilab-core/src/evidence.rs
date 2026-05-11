//! Stable `kind` strings for append-only evidence rows (business ledger + runtime alignment).

/// Evidence kinds used in week-1 workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceKind;

impl EvidenceKind {
    pub const TRIGGER_INGESTED: &'static str = "trigger.ingested";
    pub const ACCOUNT_CREATED: &'static str = "account.created";
    pub const ACCOUNT_MERGED: &'static str = "account.merged";
    pub const ACCOUNT_SCORED: &'static str = "account.scored";
    pub const BUYING_SIGNAL_OBSERVED: &'static str = "buying_signal.observed";
    pub const MESSAGE_FAMILY_CREATED: &'static str = "message_family.created";
    pub const OUTREACH_SENT: &'static str = "outreach.sent";
    pub const REPLY_RECEIVED: &'static str = "reply.received";
    pub const REPLY_CLASSIFIED: &'static str = "reply.classified";
    pub const CONTACT_OPTED_OUT: &'static str = "contact.opted_out";
    pub const MEETING_SCHEDULED: &'static str = "meeting.scheduled";
    pub const MEETING_COMPLETED: &'static str = "meeting.completed";
    pub const POLICY_BLOCKED: &'static str = "policy.blocked";
    pub const DEPARTMENT_RUN_STARTED: &'static str = "department_run.started";
    pub const DEPARTMENT_RUN_COMPLETED: &'static str = "department_run.completed";
    pub const DEPARTMENT_RUN_FAILED: &'static str = "department_run.failed";
    pub const PREMIUM_EXCEPTION_RAISED: &'static str = "premium_exception.raised";

    // ---- huge-into-grammar vertical (outbound.send slice) -----------------
    // These kinds express the constitutional chain for a single outbound act:
    // eligibility → premium → policy → admitted → sent | failed, plus the
    // canonical opt-out assertion emitted during reply ingestion. They coexist
    // with the legacy kinds above (OUTREACH_SENT, POLICY_BLOCKED, CONTACT_OPTED_OUT)
    // which remain authoritative for the existing sequence-based flow during
    // the transition. See `docs/integration/huge-into-grammar.md` section 4.
    pub const ELIGIBILITY_EVALUATED: &'static str = "eligibility.evaluated";
    pub const PREMIUM_EVALUATED: &'static str = "premium.evaluated";
    pub const POLICY_EVALUATED: &'static str = "policy.evaluated";
    pub const OUTBOUND_ADMITTED: &'static str = "outbound.admitted";
    pub const OUTBOUND_SENT: &'static str = "outbound.sent";
    pub const OUTBOUND_FAILED: &'static str = "outbound.failed";
    pub const OPTOUT_ASSERTED: &'static str = "optout.asserted";

    // ---- bundle-into-grammar vertical (host.pair slice) -------------------
    // Constitutional chain for the first physical-edge act:
    //   host.pair.initiated (pre-execution) → host.paired | host.pair_failed.
    // See `docs/integration/bundle-into-grammar.md` §4. These coexist with all
    // other evidence kinds in the single `evidence_ledger` per invariant #2
    // ("no parallel evidence store").
    pub const HOST_PAIR_INITIATED: &'static str = "host.pair.initiated";
    pub const HOST_PAIRED: &'static str = "host.paired";
    pub const HOST_PAIR_FAILED: &'static str = "host.pair_failed";

    // ---- bundle-into-grammar vertical (install.reconcile slice) ------------
    // Reconcile-shaped act: plan desired/applied convergence, apply each
    // divergent service step, then close as reconciled or failed.
    pub const INSTALL_RECONCILE_PLANNED: &'static str = "install.reconcile.planned";
    pub const INSTALL_RECONCILE_STEP_APPLIED: &'static str = "install.reconcile.step.applied";
    pub const INSTALL_RECONCILE_RECONCILED: &'static str = "install.reconcile.reconciled";
    pub const INSTALL_RECONCILE_FAILED: &'static str = "install.reconcile.failed";
}
