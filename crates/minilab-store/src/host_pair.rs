//! Constitutional `host.pair` orchestrator.
//!
//! First slice of the bundle-into-grammar ADR (§4). Composes the admissibility
//! chain for a single platform-side pairing act, producing a complete
//! constitutional lineage in the evidence ledger under one `correlation_id`:
//!
//! ```text
//!   host.pair.initiated      (pre-execution)
//!     → host.paired          (success)
//!     | host.pair_failed     (rejection — either pre-admission or execution)
//! ```
//!
//! Input is intentionally the minimum to express the act:
//! `{ host_id, challenge, agent_pubkey, agent_signature?, correlation_id }`.
//!
//! # Design memory (slice decisions, April 2026)
//!
//! - **Replay is a deterministic rejection, not silent idempotency.** If a
//!   `host.pair.initiated` row already exists for the same
//!   `(host_id, challenge_hash)`, the orchestrator writes
//!   `host.pair_failed { reason_code: "replay_detected" }` — it does **not**
//!   re-emit `initiated`. The ledger's own unique-key protection remains as a
//!   safety net below this semantic rejection.
//! - **Dispatcher landed.** The live act-shaped API now lowers
//!   `ActionKind::Canonical(CanonicalActionId("host.pair"))` and routes the resulting
//!   `OperationalCommand { namespace: "host", verb: "pair",
//!   target_runtime: Platform }` through `dispatch_operational_command`
//!   before it reaches this orchestrator. This module remains the typed
//!   constitutional composer of the slice.
//! - **Executor split.** Under non-production `sim_mode` a mock platform
//!   pairer may run. Under `SimMode::Production` the executor verifies an
//!   ed25519 signature over the challenge with the submitted agent public key;
//!   missing or invalid signatures fail closed at `phase: execution`.
//! - **Capability stub.** Presence of the `host` row stands in for capability
//!   presence in this slice; the real `CapabilityManifest` binding is
//!   deferred. Do **not** read this as the final constitutional meaning of
//!   capability — it is a temporary bridge.
//! - **Policy class C escalation deferred.** When `pairing_token_hash` is
//!   absent the ADR prescribes escalating to `Confirm` from Compliance/Founder.
//!   This slice instead rejects with `token_mismatch` under
//!   `phase: pre_admission`. The escalation path is a follow-up vertical.
//! - **No IR-level host.register / host.retire yet.** Only the pair act.
//!
//! Failure payload always carries a `phase` field so downstream audit can
//! distinguish admissibility rejection from execution error:
//!
//! - `pre_admission`: `host_not_registered`, `host_retired`, `token_mismatch`,
//!   `version_inconsistent`, `replay_detected`.
//! - `execution`:     `invalid_challenge`, `invalid_signature`, `upstream_error`.

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use minilab_core::evidence::EvidenceKind;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;

/// Minimal input for the constitutional `host.pair` chain.
#[derive(Debug, Clone)]
pub struct HostPairInput {
    pub host_id: Uuid,
    /// Base64-encoded nonce supplied by the agent side.
    pub challenge: String,
    /// Base64-encoded ed25519 public key the agent claims for this session.
    pub agent_pubkey: String,
    /// Base64-encoded ed25519 signature over `challenge`. Required in Production.
    pub agent_signature: Option<String>,
    pub correlation_id: Uuid,
}

/// Terminal outcome of [`submit_host_pair`]. Every branch has closed evidence.
#[derive(Debug, Clone)]
pub enum HostPairOutcome {
    Paired {
        pairing_session_id: Uuid,
    },
    Failed {
        reason_code: String,
        reason_detail: String,
        phase: &'static str,
    },
}

impl HostPairOutcome {
    pub fn was_paired(&self) -> bool {
        matches!(self, Self::Paired { .. })
    }
}

/// Run the full admissibility chain and, if admitted, execute the (mock)
/// pairing. Never panics on an admissible path.
pub async fn submit_host_pair(
    client: &StoreClient,
    input: HostPairInput,
) -> Result<HostPairOutcome, StoreError> {
    let challenge_hash = hex_sha256(input.challenge.as_bytes());

    // --- 1. Fetch host row --------------------------------------------------
    let host = match fetch_host(client, input.host_id).await? {
        Some(row) => row,
        None => {
            return fail(
                client,
                &input,
                &challenge_hash,
                "host_not_registered",
                &format!("host {} has no registration row", input.host_id),
                "pre_admission",
            )
            .await;
        }
    };

    if host.retired_at.is_some() {
        return fail(
            client,
            &input,
            &challenge_hash,
            "host_retired",
            &format!(
                "host {} retired at {}",
                input.host_id,
                host.retired_at.as_deref().unwrap_or("?")
            ),
            "pre_admission",
        )
        .await;
    }

    // --- 2. Capability check (slice stub) -----------------------------------
    // Slice stub: host existence stands in for capability presence until
    // host capability declarations are modeled explicitly. This is NOT the
    // final constitutional meaning of capability (bundle ADR §1:
    // "Capability is not authority" — both still need to be expressed
    // separately). A real CapabilityManifest binding for `host.pair` is a
    // deferred vertical.
    //
    // No branch is needed here today because existence was already verified
    // at step 1; the comment documents the deliberate shortcut.

    // --- 3. Pre-authorization token check -----------------------------------
    if host.pairing_token_hash.as_deref().unwrap_or("").is_empty() {
        return fail(
            client,
            &input,
            &challenge_hash,
            "token_mismatch",
            "host has no pairing_token_hash set (Policy class C escalation to Confirm is deferred)",
            "pre_admission",
        )
        .await;
    }

    // --- 4. Version consistency check ---------------------------------------
    if host.canon_version.is_empty()
        || host.elastic_version.is_empty()
        || host.canon_version == "unknown"
        || host.elastic_version == "unknown"
    {
        return fail(
            client,
            &input,
            &challenge_hash,
            "version_inconsistent",
            &format!(
                "canon_version={} elastic_version={} must both be set",
                host.canon_version, host.elastic_version
            ),
            "pre_admission",
        )
        .await;
    }

    // --- 5. Replay check (deterministic rejection, not silent idempotency) --
    if prior_initiated_exists(client, input.host_id, &challenge_hash).await? {
        return fail(
            client,
            &input,
            &challenge_hash,
            "replay_detected",
            "this (host_id, challenge) pair was already initiated in a prior correlation",
            "pre_admission",
        )
        .await;
    }

    // --- 6. Record admission: host.pair.initiated ---------------------------
    insert_ledger_row(
        client,
        EvidenceKind::HOST_PAIR_INITIATED,
        json!({
            "host_id": input.host_id,
            "challenge_hash": challenge_hash,
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "host.pair.initiated:{}:{}",
            input.host_id, challenge_hash
        )),
    )
    .await?;

    // --- 7. Executor selection ---------------------------------------------
    if !client.sim_mode.blocks_real_sends() {
        if let Err(detail) = verify_ed25519_pairing(
            &input.challenge,
            &input.agent_pubkey,
            input.agent_signature.as_deref(),
        ) {
            return fail(
                client,
                &input,
                &challenge_hash,
                "invalid_signature",
                &detail,
                "execution",
            )
            .await;
        }
    }

    // --- 8. Mock/real execution closure ------------------------------------
    if !is_plausible_base64(&input.challenge) || !is_plausible_base64(&input.agent_pubkey) {
        return fail(
            client,
            &input,
            &challenge_hash,
            "invalid_challenge",
            "challenge or agent_pubkey failed shape validation (non-base64 or empty)",
            "execution",
        )
        .await;
    }

    let pairing_session_id = Uuid::new_v4();
    let paired_at = chrono::Utc::now();

    insert_ledger_row(
        client,
        EvidenceKind::HOST_PAIRED,
        json!({
            "host_id": input.host_id,
            "agent_pubkey": input.agent_pubkey,
            "signature_verified": !client.sim_mode.blocks_real_sends(),
            "pairing_session_id": pairing_session_id,
            "paired_at": paired_at,
            "canon_version": host.canon_version,
            "elastic_version": host.elastic_version,
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "host.paired:{}:{}",
            input.host_id, pairing_session_id
        )),
    )
    .await?;

    Ok(HostPairOutcome::Paired { pairing_session_id })
}

// ----------------------------------------------------------------------------
// Internals
// ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct HostRow {
    retired_at: Option<String>,
    pairing_token_hash: Option<String>,
    canon_version: String,
    elastic_version: String,
}

async fn fetch_host(client: &StoreClient, host_id: Uuid) -> Result<Option<HostRow>, StoreError> {
    let resp = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=retired_at,pairing_token_hash,canon_version,elastic_version&limit=1",
            client.rest("host"),
            host_id
        ))
        .send()
        .await?;

    let status = resp.status().as_u16();
    let body = resp.text().await?;
    if status >= 300 {
        return Err(StoreError::Supabase { status, body });
    }
    let rows: Vec<Value> = serde_json::from_str(&body).map_err(|err| {
        StoreError::Contract(format!("host lookup returned invalid JSON: {err}: {body}"))
    })?;

    Ok(rows.into_iter().next().map(|r| HostRow {
        retired_at: r["retired_at"].as_str().map(str::to_owned),
        pairing_token_hash: r["pairing_token_hash"].as_str().map(str::to_owned),
        canon_version: r["canon_version"].as_str().unwrap_or("").to_owned(),
        elastic_version: r["elastic_version"].as_str().unwrap_or("").to_owned(),
    }))
}

/// Scan the ledger for a prior `host.pair.initiated` row covering this
/// `(host_id, challenge_hash)` pair. Any such row means replay.
async fn prior_initiated_exists(
    client: &StoreClient,
    host_id: Uuid,
    challenge_hash: &str,
) -> Result<bool, StoreError> {
    let resp = client
        .http
        .get(format!(
            "{}?kind=eq.host.pair.initiated&order=created_at.desc&limit=50",
            client.rest("evidence_ledger")
        ))
        .send()
        .await?;

    let status = resp.status().as_u16();
    let body = resp.text().await?;
    if status >= 300 {
        return Err(StoreError::Supabase { status, body });
    }
    let rows: Vec<Value> = serde_json::from_str(&body).map_err(|err| {
        StoreError::Contract(format!(
            "evidence replay lookup returned invalid JSON: {err}: {body}"
        ))
    })?;

    for row in rows {
        let payload = &row["payload"];
        if payload["host_id"].as_str() == Some(host_id.to_string().as_str())
            && payload["challenge_hash"].as_str() == Some(challenge_hash)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn fail(
    client: &StoreClient,
    input: &HostPairInput,
    challenge_hash: &str,
    reason_code: &'static str,
    reason_detail: &str,
    phase: &'static str,
) -> Result<HostPairOutcome, StoreError> {
    insert_ledger_row(
        client,
        EvidenceKind::HOST_PAIR_FAILED,
        json!({
            "host_id": input.host_id,
            "challenge_hash": challenge_hash,
            "reason_code": reason_code,
            "reason_detail": reason_detail,
            "phase": phase,
            "correlation_id": input.correlation_id,
        }),
        input.correlation_id,
        Some(format!(
            "host.pair_failed:{}:{}:{}",
            input.host_id, challenge_hash, reason_code
        )),
    )
    .await?;

    Ok(HostPairOutcome::Failed {
        reason_code: reason_code.into(),
        reason_detail: reason_detail.into(),
        phase,
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Cheap shape check for base64-ish input. Rejects empty strings and ones
/// whose characters are entirely outside the base64 alphabet. Does **not**
/// decode — the mock executor is not responsible for cryptographic validity.
fn verify_ed25519_pairing(
    challenge: &str,
    agent_pubkey: &str,
    agent_signature: Option<&str>,
) -> Result<(), String> {
    let signature = agent_signature.ok_or_else(|| {
        "production host.pair requires agent_signature over challenge".to_string()
    })?;
    let challenge_bytes =
        decode_base64(challenge).map_err(|err| format!("challenge is not valid base64: {err}"))?;
    let pubkey_bytes = decode_base64(agent_pubkey)
        .map_err(|err| format!("agent_pubkey is not valid base64: {err}"))?;
    let signature_bytes = decode_base64(signature)
        .map_err(|err| format!("agent_signature is not valid base64: {err}"))?;

    let pubkey_array: [u8; 32] = pubkey_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "agent_pubkey must decode to 32 bytes".to_string())?;
    let signature_array: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "agent_signature must decode to 64 bytes".to_string())?;

    let verifying_key = VerifyingKey::from_bytes(&pubkey_array)
        .map_err(|err| format!("agent_pubkey is not an ed25519 verifying key: {err}"))?;
    let signature = Signature::from_bytes(&signature_array);
    verifying_key
        .verify(&challenge_bytes, &signature)
        .map_err(|err| format!("ed25519 signature verification failed: {err}"))
}

fn decode_base64(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    general_purpose::STANDARD
        .decode(input)
        .or_else(|_| general_purpose::URL_SAFE.decode(input))
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(input))
}

fn is_plausible_base64(s: &str) -> bool {
    if s.is_empty() || s.len() < 4 {
        return false;
    }
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_'
    })
}

#[cfg(test)]
mod tests {
    use super::{hex_sha256, is_plausible_base64, verify_ed25519_pairing};

    #[test]
    fn challenge_hash_is_stable_hex_sha256() {
        // Known SHA-256 of the ASCII string "nonce".
        assert_eq!(
            hex_sha256(b"nonce"),
            "78377b525757b494427f89014f97d79928f3938d14eb51e20fb5dec9834eb304"
        );
    }

    #[test]
    fn production_pairing_requires_signature() {
        let err = verify_ed25519_pairing(
            "bm9uY2U=",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            None,
        )
        .expect_err("missing signature must fail");
        assert!(err.contains("requires agent_signature"));
    }

    #[test]
    fn plausible_base64_rejects_empty_and_short() {
        assert!(!is_plausible_base64(""));
        assert!(!is_plausible_base64("ab"));
        assert!(is_plausible_base64("bm9uY2U="));
        assert!(!is_plausible_base64("bm9uY 2U=")); // contains space
    }
}
