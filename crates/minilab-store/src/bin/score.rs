/// minilab-score — score all unscored accounts, attach a message family to top scorers.
///
/// Usage:
///   SUPABASE_URL=https://... SUPABASE_SECRET_KEY=... cargo run --bin score
use minilab_store::{
    campaign::{create_message_family, ensure_campaign, ensure_claims_profile},
    scoring::score_account,
    StoreClient,
};
use serde_json::Value;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let client = StoreClient::from_env().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    // Fetch all accounts ordered by score ascending (score=0 = unscored first).
    let accounts: Vec<Value> = client
        .http
        .get(format!(
            "{}?select=id,name,domain,score&order=score.asc&limit=50",
            client.rest("account")
        ))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("fetch accounts: {e}");
            std::process::exit(1)
        })
        .json::<Vec<Value>>()
        .await
        .unwrap_or_default();
    println!("scoring {} accounts…", accounts.len());

    // Ensure shared campaign infrastructure.
    let cp_id = ensure_claims_profile(&client, "default_v1")
        .await
        .unwrap_or_else(|e| {
            eprintln!("claims_profile: {e}");
            std::process::exit(1)
        });

    let campaign_id = ensure_campaign(&client, "wedge_email_v1", "email", cp_id)
        .await
        .unwrap_or_else(|e| {
            eprintln!("campaign: {e}");
            std::process::exit(1)
        });

    for acct in &accounts {
        let account_id: Uuid = match serde_json::from_value(acct["id"].clone()) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let correlation_id = Uuid::new_v4();

        match score_account(&client, account_id, correlation_id).await {
            Ok(score) => {
                println!(
                    "  {} ({}) → score {:.0}",
                    acct["name"].as_str().unwrap_or("?"),
                    acct["domain"].as_str().unwrap_or("?"),
                    score
                );

                // Attach a message family to accounts scoring ≥ 70.
                if score >= 70.0 {
                    match create_message_family(
                        &client,
                        campaign_id,
                        "angle_pain",
                        "templates/pain_v1.md",
                        correlation_id,
                    )
                    .await
                    {
                        Ok(fid) => println!("    → message_family {fid}"),
                        Err(e) => eprintln!("    message_family error: {e}"),
                    }
                }
            }
            Err(e) => eprintln!("  score error for {account_id}: {e}"),
        }
    }

    println!("done.");
}
