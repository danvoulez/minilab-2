//! Supabase-compatible **REST** evidence sink (PostgREST row insert).
//!
//! Table shape expected (you create this in Supabase SQL):
//!
//! ```sql
//! create table if not exists evidence_records (
//!   id bigint generated always as identity primary key,
//!   kind text not null,
//!   payload jsonb not null,
//!   created_at timestamptz default now()
//! );
//! ```
//!
//! `rest_v1_base` is the full prefix ending in `/rest/v1`, e.g.
//! `https://<project>.supabase.co/rest/v1`.

use crate::evidence::{EvidenceRecord, EvidenceStore, EvidenceStoreError};
use reqwest::blocking::Client;
use serde_json::json;

/// Insert one row per evidence record via `POST /rest/v1/<table>`.
#[derive(Clone, Debug)]
pub struct SupabaseRestEvidenceStore {
    client: Client,
    post_url: String,
    apikey: String,
}

impl SupabaseRestEvidenceStore {
    pub fn new(
        rest_v1_base: impl AsRef<str>,
        table: impl AsRef<str>,
        apikey: impl AsRef<str>,
    ) -> Self {
        let base = rest_v1_base.as_ref().trim_end_matches('/');
        let table = table.as_ref().trim_matches('/');
        let post_url = format!("{}/{}", base, table);
        Self {
            client: Client::new(),
            post_url,
            apikey: apikey.as_ref().to_string(),
        }
    }
}

impl EvidenceStore for SupabaseRestEvidenceStore {
    fn write_record(&self, record: EvidenceRecord) -> Result<(), EvidenceStoreError> {
        let body = json!({
            "kind": record.kind,
            "payload": record.payload_json,
        });
        let res = self
            .client
            .post(&self.post_url)
            .header("apikey", &self.apikey)
            .header("Authorization", format!("Bearer {}", self.apikey))
            .header("Content-Type", "application/json")
            .header("Prefer", "return=minimal")
            .json(&body)
            .send()
            .map_err(|e| EvidenceStoreError::Http(e.to_string()))?;
        let status = res.status();
        let text = res
            .text()
            .map_err(|e| EvidenceStoreError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(EvidenceStoreError::Http(format!(
                "status {} body: {}",
                status, text
            )));
        }
        Ok(())
    }
}
