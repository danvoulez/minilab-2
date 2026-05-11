/// minilab-send-due — run the policy gate on all pending sequences, then block
/// or dispatch through the configured outbound provider.
///
/// Usage:
///   SUPABASE_URL=https://... SUPABASE_SECRET_KEY=... cargo run --bin send_due
use minilab_store::{
    outbound::dispatch_sequence,
    outreach::{block_send, record_successful_send},
    policy::{evaluate_send_policy, PolicyDecision},
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

    // Fetch all pending sequences due now.
    let sequences: Vec<Value> = client
        .http
        .get(format!(
            "{}?state=eq.pending&scheduled_at=lte.{}&select=*&order=scheduled_at.asc&limit=100",
            client.rest("sequence"),
            chrono::Utc::now().to_rfc3339()
        ))
        .send()
        .await
        .unwrap_or_else(|e| {
            eprintln!("fetch sequences: {e}");
            std::process::exit(1)
        })
        .json::<Vec<Value>>()
        .await
        .unwrap_or_default();

    println!("processing {} pending sequence(s)…", sequences.len());

    for seq in &sequences {
        let sequence_id: Uuid = serde_json::from_value(seq["id"].clone()).unwrap();
        let contact_id: Uuid = serde_json::from_value(seq["contact_id"].clone()).unwrap();
        let account_id: Uuid = serde_json::from_value(seq["account_id"].clone()).unwrap();
        let campaign_id: Uuid = serde_json::from_value(seq["campaign_id"].clone()).unwrap();
        let correlation_id = Uuid::new_v4();

        match evaluate_send_policy(&client, contact_id, account_id).await {
            Ok(PolicyDecision::Allowed) => {
                match dispatch_sequence(
                    &client,
                    sequence_id,
                    account_id,
                    contact_id,
                    campaign_id,
                    correlation_id,
                )
                .await
                {
                    Ok(receipt) => match record_successful_send(
                        &client,
                        sequence_id,
                        account_id,
                        contact_id,
                        campaign_id,
                        correlation_id,
                        &receipt,
                    )
                    .await
                    {
                        Ok(_) => println!(
                            "  ✓ sent   sequence={sequence_id} provider={} channel={}",
                            receipt.provider, receipt.channel
                        ),
                        Err(e) => eprintln!("  record_successful_send error: {e}"),
                    },
                    Err(e) => eprintln!("  dispatch_sequence error: {e}"),
                }
            }
            Ok(PolicyDecision::Blocked { reason }) => {
                match block_send(&client, sequence_id, contact_id, &reason, correlation_id).await {
                    Ok(_) => println!("  ✗ blocked sequence={sequence_id} reason={reason}"),
                    Err(e) => eprintln!("  block_send error: {e}"),
                }
            }
            Err(e) => eprintln!("  policy eval error: {e}"),
        }
    }

    println!("done.");
}
