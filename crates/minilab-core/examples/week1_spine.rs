//! Simulated week-1 spine: prints departments, workflow order, and example evidence kinds.
//!
//! Run: `cargo run -p minilab-core --example week1_spine`

use chrono::Utc;
use minilab_core::entities::{Account, Campaign, CampaignStatus, ClaimsProfile, Contact};
use minilab_core::{Department, EvidenceKind, Week1Step};
use serde_json::json;
use uuid::Uuid;

fn main() {
    println!("Minilab — departments (jurisdictions)");
    for d in [
        Department::Research,
        Department::SalesOps,
        Department::Marketing,
        Department::Sdr,
        Department::Compliance,
        Department::MemoryAnalytics,
    ] {
        println!("  {:20} {}", d.as_str(), d);
    }

    println!("\nWeek-1 spine (ship order)");
    for step in Week1Step::ORDER {
        let k = step
            .example_kind()
            .unwrap_or("(correlation-level closure — multiple kinds)");
        println!("  {:?} → {}", step, k);
    }

    println!("\nEvidence kinds (constants)");
    println!("  {}", EvidenceKind::TRIGGER_INGESTED);
    println!("  {}", EvidenceKind::POLICY_BLOCKED);
    println!("  {}", EvidenceKind::OUTREACH_SENT);

    let now = Utc::now();
    let account_id = Uuid::new_v4();
    let claims_id = Uuid::new_v4();
    let campaign_id = Uuid::new_v4();

    let account = Account {
        id: account_id,
        name: "Acme Corp".into(),
        domain: "acme.example".into(),
        icp_segment: "wedge_icp_v1".into(),
        score: 0.42,
        stage: "working".into(),
        strategic_flag: false,
        dedupe_key: "acme.example".into(),
        metadata: json!({ "source": "week1_spine" }),
        created_at: now,
        updated_at: now,
    };

    let contact = Contact {
        id: Uuid::new_v4(),
        account_id,
        email: "buyer@acme.example".into(),
        phone_e164: None,
        role_inferred: Some("economic_buyer".into()),
        buyer_persona_id: Some("persona_v1".into()),
        opt_out: false,
        opt_out_at: None,
        created_at: now,
        updated_at: now,
    };

    let _claims = ClaimsProfile {
        id: claims_id,
        name: "Wedge v1".into(),
        claim_ids: vec!["claim_product_fit".into()],
        version: 1,
        active: true,
        created_at: now,
    };

    let _campaign = Campaign {
        id: campaign_id,
        name: "Outbound wedge".into(),
        channel: "email".into(),
        status: CampaignStatus::Active,
        claims_profile_id: claims_id,
        created_at: now,
        updated_at: now,
    };

    println!("\nSample rows (JSON snapshot)");
    println!("{}", serde_json::to_string_pretty(&account).unwrap());
    println!("{}", serde_json::to_string_pretty(&contact).unwrap());
}
