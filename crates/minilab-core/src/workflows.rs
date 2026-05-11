use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::business_canon::DepartmentKind;
use crate::elastic_config::{CampaignSpec, ElasticOperatingConfig};
use crate::{Priority, WorkPlan, WorkTarget};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowKind {
    MarketSensing,
    AccountGraphRefresh,
    CampaignCompile,
    OutreachExecution,
    ReplyClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSpec {
    pub kind: WorkflowKind,
    pub owner: DepartmentKind,
    pub inputs: Vec<WorkflowInput>,
    pub outputs: Vec<WorkflowOutput>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowInput {
    TriggerSources(Vec<String>),
    TriggerEvents,
    RankedAccounts,
    Campaign(String),
    DueSequenceSteps,
    IncomingReplies,
    ClaimsProfile(String),
    ActiveOffer(String),
    ActiveIcp(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowOutput {
    TriggerEvents,
    AccountUpdates,
    ContactUpdates,
    BuyingSignals,
    CampaignAssets,
    OutboundSends,
    ReplyRecords,
    MeetingBookings,
    EvidenceRefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: String,
    pub kind: WorkflowKind,
    pub status: WorkflowStatus,
    pub work_plans: Vec<WorkPlan>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Blocked,
}

pub fn workflow_specs_for_config(config: &ElasticOperatingConfig) -> Vec<WorkflowSpec> {
    let mut specs = Vec::new();

    specs.push(WorkflowSpec {
        kind: WorkflowKind::MarketSensing,
        owner: DepartmentKind::Research,
        inputs: vec![WorkflowInput::TriggerSources(config.icp.triggers.clone())],
        outputs: vec![WorkflowOutput::TriggerEvents, WorkflowOutput::EvidenceRefs],
        notes: vec!["observe and collect market triggers for active wedge".to_string()],
    });

    specs.push(WorkflowSpec {
        kind: WorkflowKind::AccountGraphRefresh,
        owner: DepartmentKind::SalesOps,
        inputs: vec![WorkflowInput::TriggerEvents],
        outputs: vec![
            WorkflowOutput::AccountUpdates,
            WorkflowOutput::ContactUpdates,
            WorkflowOutput::BuyingSignals,
            WorkflowOutput::EvidenceRefs,
        ],
        notes: vec!["refresh account/contact graph and score active opportunities".to_string()],
    });

    for campaign in config.active_campaigns() {
        specs.push(WorkflowSpec {
            kind: WorkflowKind::CampaignCompile,
            owner: DepartmentKind::Marketing,
            inputs: vec![
                WorkflowInput::Campaign(campaign.id.clone()),
                WorkflowInput::ClaimsProfile(config.claims_profile_id.clone()),
                WorkflowInput::ActiveOffer(config.offer.id.clone()),
                WorkflowInput::ActiveIcp(config.icp.segment.clone()),
            ],
            outputs: vec![WorkflowOutput::CampaignAssets, WorkflowOutput::EvidenceRefs],
            notes: vec!["compile approved outbound assets inside claims profile".to_string()],
        });

        specs.push(WorkflowSpec {
            kind: WorkflowKind::OutreachExecution,
            owner: DepartmentKind::Sdr,
            inputs: vec![
                WorkflowInput::Campaign(campaign.id.clone()),
                WorkflowInput::DueSequenceSteps,
            ],
            outputs: vec![WorkflowOutput::OutboundSends, WorkflowOutput::EvidenceRefs],
            notes: vec!["execute due outbound steps under channel policy".to_string()],
        });
    }

    specs.push(WorkflowSpec {
        kind: WorkflowKind::ReplyClassification,
        owner: DepartmentKind::Sdr,
        inputs: vec![WorkflowInput::IncomingReplies],
        outputs: vec![
            WorkflowOutput::ReplyRecords,
            WorkflowOutput::MeetingBookings,
            WorkflowOutput::EvidenceRefs,
        ],
        notes: vec!["classify replies and route outcomes".to_string()],
    });

    specs
}

pub fn work_plans_for_workflow(
    config: &ElasticOperatingConfig,
    spec: &WorkflowSpec,
) -> Vec<WorkPlan> {
    match spec.kind {
        WorkflowKind::MarketSensing => vec![WorkPlan {
            department: DepartmentKind::Research,
            primitive: "COLLECT".to_string(),
            target: WorkTarget::Company,
            priority: Priority::Normal,
            inputs: config.icp.triggers.clone(),
            notes: vec!["collect market events and triggers".to_string()],
        }],
        WorkflowKind::AccountGraphRefresh => vec![
            WorkPlan {
                department: DepartmentKind::SalesOps,
                primitive: "FETCH".to_string(),
                target: WorkTarget::Company,
                priority: Priority::Normal,
                inputs: vec!["trigger_events".to_string()],
                notes: vec!["load existing accounts and contacts".to_string()],
            },
            WorkPlan {
                department: DepartmentKind::SalesOps,
                primitive: "CLASSIFY".to_string(),
                target: WorkTarget::Company,
                priority: Priority::High,
                inputs: vec![config.icp.segment.clone(), config.offer.persona.clone()],
                notes: vec!["classify and score buying signals".to_string()],
            },
            WorkPlan {
                department: DepartmentKind::SalesOps,
                primitive: "PERSIST".to_string(),
                target: WorkTarget::Company,
                priority: Priority::Normal,
                inputs: vec!["account_graph_delta".to_string()],
                notes: vec!["persist graph updates and rankings".to_string()],
            },
        ],
        WorkflowKind::CampaignCompile => {
            let campaign_id = extract_campaign_id(spec);
            vec![WorkPlan {
                department: DepartmentKind::Marketing,
                primitive: "EMIT".to_string(),
                target: WorkTarget::Campaign(campaign_id),
                priority: Priority::Normal,
                inputs: vec![
                    config.offer.id.clone(),
                    config.claims_profile_id.clone(),
                    config.icp.segment.clone(),
                ],
                notes: vec!["compile campaign assets from approved modules".to_string()],
            }]
        }
        WorkflowKind::OutreachExecution => {
            let campaign_id = extract_campaign_id(spec);
            vec![
                WorkPlan {
                    department: DepartmentKind::Sdr,
                    primitive: "EXECUTE".to_string(),
                    target: WorkTarget::Campaign(campaign_id.clone()),
                    priority: Priority::High,
                    inputs: vec!["due_sequence_steps".to_string()],
                    notes: vec!["send bounded outbound messages".to_string()],
                },
                WorkPlan {
                    department: DepartmentKind::Sdr,
                    primitive: "SCHEDULE".to_string(),
                    target: WorkTarget::Campaign(campaign_id),
                    priority: Priority::Normal,
                    inputs: vec!["next_followup".to_string()],
                    notes: vec!["schedule next legal follow-up or stop".to_string()],
                },
            ]
        }
        WorkflowKind::ReplyClassification => vec![
            WorkPlan {
                department: DepartmentKind::Sdr,
                primitive: "CLASSIFY".to_string(),
                target: WorkTarget::Company,
                priority: Priority::High,
                inputs: vec!["incoming_replies".to_string()],
                notes: vec!["classify positive, objection, unsubscribe, spam-risk".to_string()],
            },
            WorkPlan {
                department: DepartmentKind::Sdr,
                primitive: "ROUTE".to_string(),
                target: WorkTarget::Department(DepartmentKind::Sdr),
                priority: Priority::High,
                inputs: vec!["reply_classification".to_string()],
                notes: vec!["route meetings, stop signals, and escalations".to_string()],
            },
        ],
    }
}

pub fn workflow_runs_from_config(config: &ElasticOperatingConfig) -> Vec<WorkflowRun> {
    workflow_specs_for_config(config)
        .into_iter()
        .enumerate()
        .map(|(idx, spec)| WorkflowRun {
            id: format!("wf-{}", idx + 1),
            kind: spec.kind.clone(),
            status: WorkflowStatus::Planned,
            work_plans: work_plans_for_workflow(config, &spec),
            metadata: default_workflow_metadata(config, &spec),
        })
        .collect()
}

pub fn campaign_workflow_bundle(
    config: &ElasticOperatingConfig,
    campaign: &CampaignSpec,
) -> Vec<WorkflowRun> {
    workflow_runs_from_config(config)
        .into_iter()
        .filter(|run| match run.kind {
            WorkflowKind::CampaignCompile | WorkflowKind::OutreachExecution => run
                .work_plans
                .iter()
                .any(|p| matches!(&p.target, WorkTarget::Campaign(id) if id == &campaign.id)),
            _ => false,
        })
        .collect()
}

fn extract_campaign_id(spec: &WorkflowSpec) -> String {
    spec.inputs
        .iter()
        .find_map(|input| match input {
            WorkflowInput::Campaign(id) => Some(id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "campaign-unknown".to_string())
}

fn default_workflow_metadata(
    config: &ElasticOperatingConfig,
    spec: &WorkflowSpec,
) -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        (
            "config_id".to_string(),
            serde_json::Value::String(config.id.clone()),
        ),
        (
            "workflow_kind".to_string(),
            serde_json::Value::String(format!("{:?}", spec.kind)),
        ),
        (
            "claims_profile_id".to_string(),
            serde_json::Value::String(config.claims_profile_id.clone()),
        ),
    ])
}
