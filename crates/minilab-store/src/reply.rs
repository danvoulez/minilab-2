use base64::{engine::general_purpose::STANDARD, Engine as _};
use constitutional_runtime::{
    ActionKind, CanonicalActionId, IRPrimitive, IrNode, Lowerer, MinilabRuntimeLowerer, NodeId,
    OperationalCommand,
};
use minilab_core::{EvidenceKind, ReplyCategory};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};
use crate::evidence::insert_ledger_row;
use crate::webhook::{validate_sendgrid_signature, validate_twilio_signature};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundReplyProvider {
    TwilioSendGridParse,
    TwilioWhatsApp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendGridParsePayload {
    pub from: String,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwilioWhatsAppInboundPayload {
    #[serde(rename = "MessageSid")]
    pub message_sid: String,
    #[serde(rename = "From")]
    pub from: String,
    #[serde(rename = "To")]
    pub to: String,
    #[serde(rename = "Body")]
    pub body: Option<String>,
    #[serde(rename = "WaId")]
    pub wa_id: Option<String>,
    #[serde(rename = "ProfileName")]
    pub profile_name: Option<String>,
    #[serde(rename = "OptOutType")]
    pub opt_out_type: Option<String>,
    #[serde(rename = "OriginalRepliedMessageSid")]
    pub original_replied_message_sid: Option<String>,
    #[serde(rename = "OriginalRepliedMessageSender")]
    pub original_replied_message_sender: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReplyInput {
    pub provider: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedReplyWebhookInput {
    pub reply: IngestReplyInput,
    pub webhook_url: String,
    pub signature_header: String,
    pub timestamp_header: Option<String>,
    pub raw_body: Option<String>,
    pub raw_body_base64: Option<String>,
    pub form_params: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyIngestResult {
    pub reply_id: Uuid,
    pub correlation_id: Uuid,
    pub account_id: Uuid,
    pub contact_id: Uuid,
    pub category: ReplyCategory,
    pub classifier_ingress_level: i32,
    pub provider: String,
}

#[derive(Debug, Clone)]
struct NormalizedReply {
    provider: InboundReplyProvider,
    provider_name: &'static str,
    channel: &'static str,
    channel_message_id: String,
    from_locator: String,
    correlation_hint: Option<Uuid>,
    provider_reply_to_message_id: Option<String>,
    body: String,
    subject: Option<String>,
    raw_payload: Value,
    ingress_signal: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedReplyContext {
    account_id: Uuid,
    contact_id: Uuid,
    correlation_id: Uuid,
}

#[derive(Debug, Clone)]
struct ClassifiedReply {
    category: ReplyCategory,
    classifier_ingress_level: i32,
}

impl InboundReplyProvider {
    pub fn parse(name: &str) -> Result<Self, StoreError> {
        match name {
            "twilio_sendgrid_parse" | "sendgrid_parse" => Ok(Self::TwilioSendGridParse),
            "twilio_whatsapp" | "whatsapp" => Ok(Self::TwilioWhatsApp),
            other => Err(StoreError::Contract(format!(
                "unknown inbound reply provider `{other}`"
            ))),
        }
    }
}

pub async fn ingest_reply(
    client: &StoreClient,
    input: IngestReplyInput,
) -> Result<ReplyIngestResult, StoreError> {
    let provider = InboundReplyProvider::parse(&input.provider)?;
    let normalized = match provider {
        InboundReplyProvider::TwilioSendGridParse => {
            normalize_sendgrid_reply(serde_json::from_value(input.payload).map_err(|err| {
                StoreError::Contract(format!("invalid SendGrid parse payload: {err}"))
            })?)?
        }
        InboundReplyProvider::TwilioWhatsApp => {
            normalize_twilio_whatsapp(serde_json::from_value(input.payload).map_err(|err| {
                StoreError::Contract(format!("invalid Twilio WhatsApp payload: {err}"))
            })?)?
        }
    };

    let resolved = resolve_reply_context(client, &normalized).await?;

    insert_ledger_row(
        client,
        EvidenceKind::REPLY_RECEIVED,
        json!({
            "provider": normalized.provider_name,
            "channel": normalized.channel,
            "channel_message_id": normalized.channel_message_id,
            "from_locator": normalized.from_locator,
            "contact_id": resolved.contact_id,
            "account_id": resolved.account_id,
            "correlation_id": resolved.correlation_id,
            "raw_payload": normalized.raw_payload,
            "subject": normalized.subject,
        }),
        resolved.correlation_id,
        Some(format!("reply.received:{}", normalized.channel_message_id)),
    )
    .await?;

    let classified = classify_reply(&normalized.body, normalized.ingress_signal.as_deref());
    let reply_id = insert_reply_row(client, &normalized, &resolved, &classified).await?;
    let ingest_command = lower_command(
        "intel.reply_ingest",
        &[
            ("provider", json!(normalized.provider_name)),
            ("channel", json!(normalized.channel)),
            ("channel_message_id", json!(normalized.channel_message_id)),
        ],
        "reply-ingest",
    )?;
    let classify_command = lower_command(
        "intel.reply_classify",
        &[
            ("reply_id", json!(reply_id)),
            (
                "classifier_ingress_level",
                json!(classified.classifier_ingress_level),
            ),
            (
                "category",
                json!(format_reply_category(&classified.category)),
            ),
        ],
        "reply-classify",
    )?;

    insert_ledger_row(
        client,
        EvidenceKind::REPLY_CLASSIFIED,
        json!({
            "reply_id": reply_id,
            "provider": normalized.provider_name,
            "channel": normalized.channel,
            "category": format_reply_category(&classified.category),
            "classifier_ingress_level": classified.classifier_ingress_level,
            "account_id": resolved.account_id,
            "contact_id": resolved.contact_id,
            "correlation_id": resolved.correlation_id,
            "ingest_command": command_json(&ingest_command),
            "classify_command": command_json(&classify_command),
        }),
        resolved.correlation_id,
        Some(format!(
            "reply.classified:{}",
            normalized.channel_message_id
        )),
    )
    .await?;

    if matches!(classified.category, ReplyCategory::Unsubscribe) {
        apply_opt_out(client, &normalized, &resolved, reply_id).await?;
    }

    Ok(ReplyIngestResult {
        reply_id,
        correlation_id: resolved.correlation_id,
        account_id: resolved.account_id,
        contact_id: resolved.contact_id,
        category: classified.category,
        classifier_ingress_level: classified.classifier_ingress_level,
        provider: normalized.provider_name.into(),
    })
}

pub async fn ingest_verified_reply(
    client: &StoreClient,
    input: VerifiedReplyWebhookInput,
) -> Result<ReplyIngestResult, StoreError> {
    verify_reply_webhook(&input)?;
    ingest_reply(client, input.reply).await
}

fn normalize_sendgrid_reply(payload: SendGridParsePayload) -> Result<NormalizedReply, StoreError> {
    let from_locator = extract_email_address(&payload.from);
    if from_locator.is_empty() {
        return Err(StoreError::Contract(
            "sendgrid parse payload missing sender email".into(),
        ));
    }

    let body = payload
        .text
        .clone()
        .or(payload.html.clone())
        .map(strip_html)
        .unwrap_or_default();
    let message_id = extract_header(&payload.headers, "Message-ID")
        .or_else(|| extract_header(&payload.headers, "Message-Id"))
        .unwrap_or_else(|| format!("sendgrid-inbound:{from_locator}"));

    Ok(NormalizedReply {
        provider: InboundReplyProvider::TwilioSendGridParse,
        provider_name: "twilio_sendgrid_parse",
        channel: "email",
        channel_message_id: message_id,
        from_locator,
        correlation_hint: payload
            .to
            .as_deref()
            .and_then(extract_correlation_from_reply_address),
        provider_reply_to_message_id: None,
        body,
        subject: payload.subject.clone(),
        raw_payload: serde_json::to_value(payload).unwrap_or_else(|_| json!({})),
        ingress_signal: None,
    })
}

fn verify_reply_webhook(input: &VerifiedReplyWebhookInput) -> Result<(), StoreError> {
    match InboundReplyProvider::parse(&input.reply.provider)? {
        InboundReplyProvider::TwilioSendGridParse => verify_sendgrid_parse_webhook(input),
        InboundReplyProvider::TwilioWhatsApp => verify_twilio_whatsapp_webhook(input),
    }
}

fn verify_sendgrid_parse_webhook(input: &VerifiedReplyWebhookInput) -> Result<(), StoreError> {
    let timestamp = input.timestamp_header.as_deref().ok_or_else(|| {
        StoreError::Contract(
            "timestamp_header is required to verify SendGrid Parse webhooks".into(),
        )
    })?;
    let raw_body = verified_raw_body(input)?;
    let public_key = std::env::var("SENDGRID_PARSE_PUBLIC_KEY").map_err(|_| {
        StoreError::Contract(
            "SENDGRID_PARSE_PUBLIC_KEY is required to verify SendGrid Parse webhooks".into(),
        )
    })?;

    if validate_sendgrid_signature(&public_key, timestamp, &raw_body, &input.signature_header)? {
        Ok(())
    } else {
        Err(StoreError::Contract(
            "invalid SendGrid Parse webhook signature".into(),
        ))
    }
}

fn verify_twilio_whatsapp_webhook(input: &VerifiedReplyWebhookInput) -> Result<(), StoreError> {
    let form_params = input.form_params.as_ref().ok_or_else(|| {
        StoreError::Contract("form_params are required to verify Twilio WhatsApp webhooks".into())
    })?;
    let auth_token = std::env::var("TWILIO_AUTH_TOKEN").map_err(|_| {
        StoreError::Contract("TWILIO_AUTH_TOKEN is required to verify Twilio webhooks".into())
    })?;

    if validate_twilio_signature(
        &auth_token,
        &input.webhook_url,
        form_params,
        &input.signature_header,
    )? {
        Ok(())
    } else {
        Err(StoreError::Contract(
            "invalid Twilio WhatsApp webhook signature".into(),
        ))
    }
}

fn verified_raw_body(input: &VerifiedReplyWebhookInput) -> Result<Vec<u8>, StoreError> {
    if let Some(raw_body_base64) = &input.raw_body_base64 {
        return STANDARD.decode(raw_body_base64).map_err(|err| {
            StoreError::Contract(format!(
                "invalid raw_body_base64 for verified webhook: {err}"
            ))
        });
    }

    input
        .raw_body
        .as_ref()
        .map(|raw| raw.as_bytes().to_vec())
        .ok_or_else(|| {
            StoreError::Contract(
                "raw_body or raw_body_base64 is required to verify SendGrid Parse webhooks".into(),
            )
        })
}

fn normalize_twilio_whatsapp(
    payload: TwilioWhatsAppInboundPayload,
) -> Result<NormalizedReply, StoreError> {
    let from_locator = strip_whatsapp_prefix(&payload.from);
    if from_locator.is_empty() {
        return Err(StoreError::Contract(
            "twilio whatsapp payload missing sender address".into(),
        ));
    }

    Ok(NormalizedReply {
        provider: InboundReplyProvider::TwilioWhatsApp,
        provider_name: "twilio_whatsapp",
        channel: "whatsapp",
        channel_message_id: payload.message_sid.clone(),
        from_locator,
        correlation_hint: None,
        provider_reply_to_message_id: payload.original_replied_message_sid.clone(),
        body: payload.body.clone().unwrap_or_default(),
        subject: None,
        raw_payload: serde_json::to_value(payload.clone()).unwrap_or_else(|_| json!({})),
        ingress_signal: payload.opt_out_type.clone(),
    })
}

async fn resolve_reply_context(
    client: &StoreClient,
    normalized: &NormalizedReply,
) -> Result<ResolvedReplyContext, StoreError> {
    let (contact_id, account_id) = match normalized.provider {
        InboundReplyProvider::TwilioSendGridParse => {
            resolve_contact_by_email(client, &normalized.from_locator).await?
        }
        InboundReplyProvider::TwilioWhatsApp => {
            resolve_contact_by_phone(client, &normalized.from_locator).await?
        }
    };

    let correlation_id = if let Some(provider_message_id) =
        normalized.provider_reply_to_message_id.as_deref()
    {
        let resolved = resolve_outreach_context_by_provider_message_id(
            client,
            provider_message_id,
            normalized.channel,
        )
        .await?;
        if resolved.contact_id != contact_id || resolved.account_id != account_id {
            return Err(StoreError::Contract(format!(
                "provider reply context {} does not match sender identity for contact {}",
                provider_message_id, contact_id
            )));
        }
        resolved.correlation_id
    } else if let Some(correlation_hint) = normalized.correlation_hint {
        let hinted =
            resolve_outreach_context_by_correlation(client, correlation_hint, normalized.channel)
                .await?;
        if hinted.contact_id != contact_id || hinted.account_id != account_id {
            return Err(StoreError::Contract(format!(
                "reply correlation hint {} does not match sender identity for contact {}",
                correlation_hint, contact_id
            )));
        }
        correlation_hint
    } else {
        find_latest_outreach_correlation(client, contact_id, normalized.channel).await?
    };

    Ok(ResolvedReplyContext {
        account_id,
        contact_id,
        correlation_id,
    })
}

async fn resolve_contact_by_email(
    client: &StoreClient,
    email: &str,
) -> Result<(Uuid, Uuid), StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(client.rest("contact"))
        .query(&vec![
            ("email", format!("eq.{email}")),
            ("select", "id,account_id".to_string()),
            ("limit", "1".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let row = rows
        .first()
        .ok_or_else(|| StoreError::Contract(format!("no contact found for email `{email}`")))?;
    let contact_id = parse_uuid_field(row, "id")?;
    let account_id = parse_uuid_field(row, "account_id")?;
    Ok((contact_id, account_id))
}

async fn resolve_contact_by_phone(
    client: &StoreClient,
    phone: &str,
) -> Result<(Uuid, Uuid), StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(client.rest("contact"))
        .query(&vec![
            ("phone_e164", format!("eq.{phone}")),
            ("select", "id,account_id".to_string()),
            ("limit", "1".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let row = rows
        .first()
        .ok_or_else(|| StoreError::Contract(format!("no contact found for phone `{phone}`")))?;
    let contact_id = parse_uuid_field(row, "id")?;
    let account_id = parse_uuid_field(row, "account_id")?;
    Ok((contact_id, account_id))
}

async fn find_latest_outreach_correlation(
    client: &StoreClient,
    contact_id: Uuid,
    channel: &str,
) -> Result<Uuid, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(client.rest("evidence_ledger"))
        .query(&vec![
            ("kind", "eq.outreach.sent".to_string()),
            ("select", "correlation_id,payload,created_at".to_string()),
            ("order", "created_at.desc".to_string()),
            ("limit", "100".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    for row in rows {
        let payload = &row["payload"];
        let payload_contact = payload["contact_id"]
            .as_str()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .or_else(|| serde_json::from_value(payload["contact_id"].clone()).ok());
        let payload_channel = payload["channel"].as_str().unwrap_or("");
        if payload_contact == Some(contact_id) && payload_channel == channel {
            return parse_uuid_field(&row, "correlation_id");
        }
    }

    Err(StoreError::Contract(format!(
        "no outreach correlation found for contact {contact_id} on channel `{channel}`"
    )))
}

async fn resolve_outreach_context_by_correlation(
    client: &StoreClient,
    correlation_id: Uuid,
    channel: &str,
) -> Result<ResolvedReplyContext, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(client.rest("evidence_ledger"))
        .query(&vec![
            ("kind", "eq.outreach.sent".to_string()),
            ("correlation_id", format!("eq.{correlation_id}")),
            ("select", "correlation_id,payload".to_string()),
            ("limit", "10".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    for row in rows {
        let payload = &row["payload"];
        if payload["channel"].as_str().unwrap_or("") != channel {
            continue;
        }
        let contact_id = payload["contact_id"]
            .as_str()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .or_else(|| serde_json::from_value(payload["contact_id"].clone()).ok())
            .ok_or_else(|| {
                StoreError::Contract(format!(
                    "outreach evidence for correlation {correlation_id} is missing contact_id"
                ))
            })?;
        let account_id = payload["account_id"]
            .as_str()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .or_else(|| serde_json::from_value(payload["account_id"].clone()).ok())
            .ok_or_else(|| {
                StoreError::Contract(format!(
                    "outreach evidence for correlation {correlation_id} is missing account_id"
                ))
            })?;
        return Ok(ResolvedReplyContext {
            account_id,
            contact_id,
            correlation_id,
        });
    }

    Err(StoreError::Contract(format!(
        "no outreach evidence found for correlation {correlation_id} on channel `{channel}`"
    )))
}

async fn resolve_outreach_context_by_provider_message_id(
    client: &StoreClient,
    provider_message_id: &str,
    channel: &str,
) -> Result<ResolvedReplyContext, StoreError> {
    let rows: Vec<Value> = client
        .http
        .get(client.rest("evidence_ledger"))
        .query(&vec![
            ("kind", "eq.outreach.sent".to_string()),
            ("select", "correlation_id,payload,created_at".to_string()),
            ("order", "created_at.desc".to_string()),
            ("limit", "100".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;

    for row in rows {
        let payload = &row["payload"];
        if payload["channel"].as_str().unwrap_or("") != channel {
            continue;
        }
        if payload["provider_message_id"].as_str().unwrap_or("") != provider_message_id {
            continue;
        }
        return Ok(ResolvedReplyContext {
            account_id: payload["account_id"]
                .as_str()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .or_else(|| serde_json::from_value(payload["account_id"].clone()).ok())
                .ok_or_else(|| {
                    StoreError::Contract(format!(
                        "outreach evidence for provider message {} is missing account_id",
                        provider_message_id
                    ))
                })?,
            contact_id: payload["contact_id"]
                .as_str()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .or_else(|| serde_json::from_value(payload["contact_id"].clone()).ok())
                .ok_or_else(|| {
                    StoreError::Contract(format!(
                        "outreach evidence for provider message {} is missing contact_id",
                        provider_message_id
                    ))
                })?,
            correlation_id: parse_uuid_field(&row, "correlation_id")?,
        });
    }

    Err(StoreError::Contract(format!(
        "no outreach evidence found for provider message {provider_message_id} on channel `{channel}`"
    )))
}

fn classify_reply(body: &str, ingress_signal: Option<&str>) -> ClassifiedReply {
    let text = body.to_ascii_lowercase();
    let signal = ingress_signal.unwrap_or("").to_ascii_uppercase();

    let category = if signal == "STOP"
        || contains_any(
            &text,
            &[
                "unsubscribe",
                "stop",
                "remove me",
                "opt out",
                "quit",
                "cancel",
            ],
        ) {
        ReplyCategory::Unsubscribe
    } else if contains_any(&text, &["spam", "reporting you", "reported"]) {
        ReplyCategory::SpamRisk
    } else if contains_any(
        &text,
        &[
            "reach out to",
            "talk to",
            "speak with",
            "contact my",
            "forward this",
        ],
    ) {
        ReplyCategory::Referral
    } else if contains_any(
        &text,
        &[
            "not interested",
            "no thanks",
            "already using",
            "not a fit",
            "too expensive",
            "budget",
            "later",
        ],
    ) {
        ReplyCategory::Objection
    } else if contains_any(
        &text,
        &["founder", "ceo", "strategic", "enterprise", "board"],
    ) {
        ReplyCategory::StrategicAccount
    } else if contains_any(
        &text,
        &[
            "interested",
            "sounds good",
            "yes",
            "let's talk",
            "book",
            "happy to chat",
        ],
    ) {
        ReplyCategory::Positive
    } else {
        ReplyCategory::Objection
    };

    ClassifiedReply {
        category,
        classifier_ingress_level: 0,
    }
}

async fn insert_reply_row(
    client: &StoreClient,
    normalized: &NormalizedReply,
    resolved: &ResolvedReplyContext,
    classified: &ClassifiedReply,
) -> Result<Uuid, StoreError> {
    let received_at = chrono::Utc::now();
    let response = client
        .http
        .post(client.rest("reply"))
        .header(
            "Prefer",
            "resolution=ignore-duplicates,return=representation",
        )
        .json(&json!({
            "account_id": resolved.account_id,
            "contact_id": resolved.contact_id,
            "correlation_id": resolved.correlation_id,
            "provider": normalized.provider_name,
            "channel_message_id": normalized.channel_message_id,
            "body": normalized.body,
            "category": format_reply_category(&classified.category),
            "classifier_ingress_level": classified.classifier_ingress_level,
            "raw_payload": normalized.raw_payload,
            "received_at": received_at,
        }))
        .send()
        .await?;

    let status = response.status().as_u16();
    if status >= 300 {
        let text = response.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let rows: Vec<Value> = response.json().await?;
    if let Some(row) = rows.first() {
        return parse_uuid_field(row, "id");
    }

    let existing_rows: Vec<Value> = client
        .http
        .get(client.rest("reply"))
        .query(&vec![
            (
                "channel_message_id",
                format!("eq.{}", normalized.channel_message_id),
            ),
            ("select", "id".to_string()),
            ("limit", "1".to_string()),
        ])
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let row = existing_rows.first().ok_or_else(|| {
        StoreError::Contract(format!(
            "reply row for {} was not returned or found",
            normalized.channel_message_id
        ))
    })?;
    parse_uuid_field(row, "id")
}

async fn apply_opt_out(
    client: &StoreClient,
    normalized: &NormalizedReply,
    resolved: &ResolvedReplyContext,
    reply_id: Uuid,
) -> Result<(), StoreError> {
    let response = client
        .http
        .patch(format!(
            "{}?id=eq.{}",
            client.rest("contact"),
            resolved.contact_id
        ))
        .json(&json!({
            "opt_out": true,
            "opt_out_at": chrono::Utc::now(),
            "updated_at": chrono::Utc::now(),
        }))
        .send()
        .await?;

    let status = response.status().as_u16();
    if status >= 300 {
        let text = response.text().await.unwrap_or_default();
        return Err(StoreError::Supabase { status, body: text });
    }

    let command = lower_command(
        "contact.apply_opt_out",
        &[
            ("contact_id", json!(resolved.contact_id)),
            ("reply_id", json!(reply_id)),
        ],
        "contact-opt-out",
    )?;

    insert_ledger_row(
        client,
        EvidenceKind::CONTACT_OPTED_OUT,
        json!({
            "contact_id": resolved.contact_id,
            "account_id": resolved.account_id,
            "reply_id": reply_id,
            "provider": normalized.provider_name,
            "channel_message_id": normalized.channel_message_id,
            "correlation_id": resolved.correlation_id,
            "command": command_json(&command),
        }),
        resolved.correlation_id,
        Some(format!(
            "contact.opted_out:{}:{}",
            resolved.contact_id, normalized.channel_message_id
        )),
    )
    .await?;

    // Canonical ADR-shape assertion. Coexists with `contact.opted_out` (legacy
    // compat) — the outbound orchestrator's opt-out gate consults both during
    // the transition. See `docs/integration/huge-into-grammar.md` §2 (opt-out
    // row) and invariant #3.
    //
    // Durability is `irreversible`: this row is never erased. A later
    // override path emits a separately-justified `optout.superseded` under
    // class D with its own lineage; the original assertion remains
    // historically authoritative.
    insert_ledger_row(
        client,
        EvidenceKind::OPTOUT_ASSERTED,
        json!({
            "contact_id": resolved.contact_id,
            "account_id": resolved.account_id,
            "channel": normalized.channel,
            "source": "reply_classified",
            "reply_id": reply_id,
            "provider": normalized.provider_name,
            "channel_message_id": normalized.channel_message_id,
            "durability": "irreversible",
            "correlation_id": resolved.correlation_id,
        }),
        resolved.correlation_id,
        Some(format!(
            "optout.asserted:{}:{}",
            resolved.contact_id, normalized.channel
        )),
    )
    .await
}

fn lower_command(
    action_name: &str,
    args: &[(&str, Value)],
    node_name: &str,
) -> Result<OperationalCommand, StoreError> {
    let mut params = Map::new();
    for (key, value) in args {
        params.insert((*key).into(), value.clone());
    }
    let action = CanonicalActionId::parse(action_name).map_err(|err| {
        StoreError::Contract(format!(
            "failed to parse reply action `{action_name}` as canonical identity: {err}"
        ))
    })?;
    let node = IrNode {
        id: NodeId(node_name.into()),
        body: IRPrimitive::Execute {
            action: ActionKind::Canonical(action),
            params,
        },
    };
    let lowerer = MinilabRuntimeLowerer;
    let (_, command) = lowerer
        .lower(&node)
        .map_err(|err| StoreError::Contract(format!("failed to lower reply command: {err}")))?;
    Ok(command)
}

fn command_json(command: &OperationalCommand) -> Value {
    json!({
        "namespace": command.namespace,
        "verb": command.verb,
        "args": command.args,
        "target_runtime": command.target_runtime,
    })
}

fn parse_uuid_field(row: &Value, key: &str) -> Result<Uuid, StoreError> {
    serde_json::from_value(row[key].clone())
        .map_err(|err| StoreError::Contract(format!("field `{key}` is not a valid UUID: {err}")))
}

fn format_reply_category(category: &ReplyCategory) -> &'static str {
    match category {
        ReplyCategory::Positive => "positive",
        ReplyCategory::Objection => "objection",
        ReplyCategory::Referral => "referral",
        ReplyCategory::Unsubscribe => "unsubscribe",
        ReplyCategory::SpamRisk => "spam_risk",
        ReplyCategory::StrategicAccount => "strategic_account",
    }
}

fn extract_email_address(value: &str) -> String {
    let trimmed = value.trim();
    if let (Some(start), Some(end)) = (trimmed.find('<'), trimmed.find('>')) {
        return trimmed[start + 1..end].trim().to_ascii_lowercase();
    }
    trimmed.trim_matches('"').trim().to_ascii_lowercase()
}

fn strip_whatsapp_prefix(value: &str) -> String {
    value
        .trim()
        .strip_prefix("whatsapp:")
        .unwrap_or(value.trim())
        .to_string()
}

fn extract_header(headers: &str, key: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (header, value) = line.split_once(':')?;
        if header.trim().eq_ignore_ascii_case(key) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn extract_correlation_from_reply_address(value: &str) -> Option<Uuid> {
    let email = extract_email_address(value);
    let (local_part, _) = email.split_once('@')?;
    let correlation_raw = local_part.split_once('+')?.1;
    Uuid::parse_str(correlation_raw).ok()
}

fn strip_html(value: String) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_unsubscribe_from_signal() {
        let classified = classify_reply("ok", Some("STOP"));
        assert!(matches!(classified.category, ReplyCategory::Unsubscribe));
    }

    #[test]
    fn classify_positive_keywords() {
        let classified = classify_reply("sounds good, let's talk next week", None);
        assert!(matches!(classified.category, ReplyCategory::Positive));
    }

    #[test]
    fn classify_referral_keywords() {
        let classified = classify_reply("please talk to our founder", None);
        assert!(matches!(classified.category, ReplyCategory::Referral));
    }

    #[test]
    fn extracts_email_address_from_header_form() {
        assert_eq!(
            extract_email_address("Jane Doe <Buyer@Acme.Example>"),
            "buyer@acme.example"
        );
    }

    #[test]
    fn strips_whatsapp_prefix() {
        assert_eq!(
            strip_whatsapp_prefix("whatsapp:+15551234567"),
            "+15551234567"
        );
    }

    #[test]
    fn sendgrid_normalization_uses_message_id_header() {
        let normalized = normalize_sendgrid_reply(SendGridParsePayload {
            from: "Buyer <buyer@acme.example>".into(),
            to: Some("reply+00000000-0000-0000-0000-000000000000@minilab.example".into()),
            subject: Some("Re: hello".into()),
            text: Some("interested".into()),
            html: None,
            headers: "Message-ID: <abc123@example.com>\nIn-Reply-To: <prior@example.com>".into(),
        })
        .unwrap();
        assert_eq!(normalized.channel_message_id, "<abc123@example.com>");
        assert_eq!(normalized.from_locator, "buyer@acme.example");
        assert_eq!(normalized.correlation_hint, Some(Uuid::nil()));
    }

    #[test]
    fn twilio_whatsapp_normalization_uses_message_sid() {
        let normalized = normalize_twilio_whatsapp(TwilioWhatsAppInboundPayload {
            message_sid: "SM123".into(),
            from: "whatsapp:+15551234567".into(),
            to: "whatsapp:+15557654321".into(),
            body: Some("STOP".into()),
            wa_id: Some("15551234567".into()),
            profile_name: Some("Buyer".into()),
            opt_out_type: Some("STOP".into()),
            original_replied_message_sid: Some("SM999".into()),
            original_replied_message_sender: Some("whatsapp:+15557654321".into()),
        })
        .unwrap();
        assert_eq!(normalized.channel_message_id, "SM123");
        assert_eq!(normalized.from_locator, "+15551234567");
        assert_eq!(
            normalized.provider_reply_to_message_id.as_deref(),
            Some("SM999")
        );
    }

    #[test]
    fn extract_correlation_from_reply_alias() {
        assert_eq!(
            extract_correlation_from_reply_address(
                "Reply <reply+00000000-0000-0000-0000-000000000000@minilab.example>"
            ),
            Some(Uuid::nil())
        );
    }

    #[test]
    fn verified_sendgrid_requires_timestamp_header() {
        let err = verify_sendgrid_parse_webhook(&VerifiedReplyWebhookInput {
            reply: IngestReplyInput {
                provider: "twilio_sendgrid_parse".into(),
                payload: json!({}),
            },
            webhook_url: "https://example.com/inbound".into(),
            signature_header: "sig".into(),
            timestamp_header: None,
            raw_body: Some("body".into()),
            raw_body_base64: None,
            form_params: None,
        })
        .unwrap_err();

        assert!(err.to_string().contains("timestamp_header"));
    }

    #[test]
    fn verified_twilio_requires_form_params() {
        let err = verify_twilio_whatsapp_webhook(&VerifiedReplyWebhookInput {
            reply: IngestReplyInput {
                provider: "twilio_whatsapp".into(),
                payload: json!({}),
            },
            webhook_url: "https://example.com/inbound".into(),
            signature_header: "sig".into(),
            timestamp_header: None,
            raw_body: None,
            raw_body_base64: None,
            form_params: None,
        })
        .unwrap_err();

        assert!(err.to_string().contains("form_params"));
    }

    #[test]
    fn verified_raw_body_decodes_base64() {
        let raw_body = verified_raw_body(&VerifiedReplyWebhookInput {
            reply: IngestReplyInput {
                provider: "twilio_sendgrid_parse".into(),
                payload: json!({}),
            },
            webhook_url: "https://example.com/inbound".into(),
            signature_header: "sig".into(),
            timestamp_header: Some("1700000000".into()),
            raw_body: None,
            raw_body_base64: Some(STANDARD.encode(b"multipart-body")),
            form_params: None,
        })
        .unwrap();

        assert_eq!(raw_body, b"multipart-body");
    }
}
