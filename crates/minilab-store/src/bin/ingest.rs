/// minilab-ingest — pipe a JSON signal, get evidence rows back.
///
/// Usage:
///   echo '{"signal_type":"job_post","source":"linkedin","domain":"acme.com",
///          "account_name":"Acme Corp","icp_segment":"saas_50_500"}' \
///     | SUPABASE_URL=https://... SUPABASE_SECRET_KEY=... cargo run --bin ingest
use minilab_store::{ingest_signal, IngestInput, StoreClient};

#[tokio::main]
async fn main() {
    let client = StoreClient::from_env().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let input: IngestInput = serde_json::from_reader(std::io::stdin()).unwrap_or_else(|e| {
        eprintln!("invalid JSON on stdin: {e}");
        std::process::exit(1);
    });

    match ingest_signal(&client, input).await {
        Ok(out) => {
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        Err(e) => {
            eprintln!("ingest failed: {e}");
            std::process::exit(1);
        }
    }
}
