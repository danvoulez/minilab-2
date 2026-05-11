/// minilab-ingest-reply — ingest a normalized provider reply payload, classify it,
/// and persist the reply plus evidence trail.
///
/// Usage:
///   echo '{"provider":"twilio_whatsapp","payload":{"MessageSid":"SM123","From":"whatsapp:+1555","To":"whatsapp:+1666","Body":"STOP"}}' \
///     | SUPABASE_URL=https://... SUPABASE_SECRET_KEY=... cargo run --bin ingest_reply
///
/// Verified webhook usage:
///   echo '{"reply":{"provider":"twilio_whatsapp","payload":{"MessageSid":"SM123","From":"whatsapp:+1555","To":"whatsapp:+1666","Body":"STOP"}},"webhook_url":"https://example.com/reply","signature_header":"...","form_params":{"Body":"STOP","From":"whatsapp:+1555","MessageSid":"SM123","To":"whatsapp:+1666"}}' \
///     | TWILIO_AUTH_TOKEN=... SUPABASE_URL=https://... SUPABASE_SECRET_KEY=... cargo run --bin ingest_reply
use minilab_store::{
    reply::{ingest_reply, ingest_verified_reply, IngestReplyInput, VerifiedReplyWebhookInput},
    StoreClient,
};
use serde_json::Value;

#[tokio::main]
async fn main() {
    let client = StoreClient::from_env().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let input: Value = serde_json::from_reader(std::io::stdin()).unwrap_or_else(|e| {
        eprintln!("invalid JSON on stdin: {e}");
        std::process::exit(1);
    });

    let result = if input.get("reply").is_some() && input.get("webhook_url").is_some() {
        let verified: VerifiedReplyWebhookInput =
            serde_json::from_value(input).unwrap_or_else(|e| {
                eprintln!("invalid verified reply JSON on stdin: {e}");
                std::process::exit(1);
            });
        ingest_verified_reply(&client, verified).await
    } else {
        let reply: IngestReplyInput = serde_json::from_value(input).unwrap_or_else(|e| {
            eprintln!("invalid reply JSON on stdin: {e}");
            std::process::exit(1);
        });
        ingest_reply(&client, reply).await
    };

    match result {
        Ok(out) => println!("{}", serde_json::to_string_pretty(&out).unwrap()),
        Err(e) => {
            eprintln!("reply ingest failed: {e}");
            std::process::exit(1);
        }
    }
}
