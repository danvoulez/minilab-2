use crate::evidence::EvidenceKind;

/// Ordered week-1 spine steps (see `docs/operations/gtm-30-day-rollout.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Week1Step {
    SignalIngest = 0,
    AccountUpsert = 1,
    AccountScore = 2,
    MessageFamily = 3,
    OutreachSend = 4,
    ReplyClassify = 5,
    EvidenceClosure = 6,
}

impl Week1Step {
    /// Steps in execution order.
    pub const ORDER: [Week1Step; 7] = [
        Week1Step::SignalIngest,
        Week1Step::AccountUpsert,
        Week1Step::AccountScore,
        Week1Step::MessageFamily,
        Week1Step::OutreachSend,
        Week1Step::ReplyClassify,
        Week1Step::EvidenceClosure,
    ];

    /// Example evidence kind for this step (where applicable).
    pub const fn example_kind(self) -> Option<&'static str> {
        match self {
            Week1Step::SignalIngest => Some(EvidenceKind::TRIGGER_INGESTED),
            Week1Step::AccountUpsert => Some(EvidenceKind::ACCOUNT_CREATED),
            Week1Step::AccountScore => Some(EvidenceKind::ACCOUNT_SCORED),
            Week1Step::MessageFamily => Some(EvidenceKind::MESSAGE_FAMILY_CREATED),
            Week1Step::OutreachSend => Some(EvidenceKind::OUTREACH_SENT),
            Week1Step::ReplyClassify => Some(EvidenceKind::REPLY_CLASSIFIED),
            Week1Step::EvidenceClosure => None,
        }
    }
}
