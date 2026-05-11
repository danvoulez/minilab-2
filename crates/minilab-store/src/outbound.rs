use constitutional_runtime::{
    ActionKind, CanonicalActionId, IRPrimitive, IrNode, Lowerer, MinilabRuntimeLowerer, NodeId,
    OperationalCommand,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::client::{StoreClient, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundProvider {
    Auto,
    Mock,
    TwilioSendGrid,
    TwilioWhatsApp,
}

#[derive(Debug, Clone)]
pub struct DeliveryReceipt {
    pub provider: &'static str,
    pub provider_message_id: String,
    pub channel: String,
    pub mock: bool,
    pub operational_command: OperationalCommand,
    pub template_ref: String,
    pub message_family_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct CampaignContext {
    campaign_id: Uuid,
    campaign_name: String,
    channel: String,
    account_id: Uuid,
    account_name: String,
    account_domain: String,
    contact_id: Uuid,
    contact_email: String,
    contact_phone_e164: Option<String>,
    sequence_id: Uuid,
    template_ref: String,
    message_family_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct RenderedMessage {
    subject: Option<String>,
    text_body: String,
    html_body: Option<String>,
}

impl OutboundProvider {
    pub fn from_env() -> Result<Self, StoreError> {
        match std::env::var("MINILAB_OUTBOUND_PROVIDER").ok().as_deref() {
            None | Some("") | Some("auto") => Ok(Self::Auto),
            Some("mock") => Ok(Self::Mock),
            Some("twilio_sendgrid") => Ok(Self::TwilioSendGrid),
            Some("twilio_whatsapp") => Ok(Self::TwilioWhatsApp),
            Some(other) => Err(StoreError::InvalidEnv {
                var: "MINILAB_OUTBOUND_PROVIDER".into(),
                value: other.into(),
            }),
        }
    }
}

fn effective_provider(
    configured: OutboundProvider,
    channel: &str,
    client: &StoreClient,
) -> Result<OutboundProvider, StoreError> {
    if client.sim_mode.blocks_real_sends() {
        return match configured {
            OutboundProvider::Mock | OutboundProvider::Auto => Ok(OutboundProvider::Mock),
            OutboundProvider::TwilioSendGrid | OutboundProvider::TwilioWhatsApp => {
                Err(StoreError::SendBlocked {
                    mode: client.sim_mode,
                })
            }
        };
    }

    match configured {
        OutboundProvider::Auto => match channel {
            "email" => Ok(OutboundProvider::TwilioSendGrid),
            "whatsapp" => Ok(OutboundProvider::TwilioWhatsApp),
            other => Err(StoreError::Contract(format!(
                "channel `{other}` has no outbound provider mapping"
            ))),
        },
        explicit => Ok(explicit),
    }
}

pub async fn dispatch_sequence(
    client: &StoreClient,
    sequence_id: Uuid,
    account_id: Uuid,
    contact_id: Uuid,
    campaign_id: Uuid,
    correlation_id: Uuid,
) -> Result<DeliveryReceipt, StoreError> {
    let context =
        fetch_campaign_context(client, sequence_id, account_id, contact_id, campaign_id).await?;
    let rendered = render_message(&context);
    let configured = OutboundProvider::from_env()?;
    let provider = effective_provider(configured, &context.channel, client)?;

    match provider {
        OutboundProvider::Mock => dispatch_mock(&context, &rendered, correlation_id),
        OutboundProvider::TwilioSendGrid => {
            dispatch_twilio_sendgrid(&context, &rendered, correlation_id).await
        }
        OutboundProvider::TwilioWhatsApp => {
            dispatch_twilio_whatsapp(&context, &rendered, correlation_id).await
        }
        OutboundProvider::Auto => Err(StoreError::Contract(
            "auto provider must resolve before dispatch".into(),
        )),
    }
}

async fn fetch_campaign_context(
    client: &StoreClient,
    sequence_id: Uuid,
    account_id: Uuid,
    contact_id: Uuid,
    campaign_id: Uuid,
) -> Result<CampaignContext, StoreError> {
    let campaign_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=id,name,channel&limit=1",
            client.rest("campaign"),
            campaign_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let campaign = campaign_rows
        .first()
        .ok_or_else(|| StoreError::Contract(format!("campaign {campaign_id} not found")))?;

    let account_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=id,name,domain&limit=1",
            client.rest("account"),
            account_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let account = account_rows
        .first()
        .ok_or_else(|| StoreError::Contract(format!("account {account_id} not found")))?;

    let contact_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?id=eq.{}&select=id,email,phone_e164,opt_out&limit=1",
            client.rest("contact"),
            contact_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let contact = contact_rows
        .first()
        .ok_or_else(|| StoreError::Contract(format!("contact {contact_id} not found")))?;

    let family_rows: Vec<Value> = client
        .http
        .get(format!(
            "{}?campaign_id=eq.{}&select=id,template_ref&order=created_at.asc&limit=1",
            client.rest("message_family"),
            campaign_id
        ))
        .send()
        .await?
        .json::<Vec<Value>>()
        .await?;
    let family = family_rows.first().ok_or_else(|| {
        StoreError::Contract(format!(
            "campaign {campaign_id} has no message_family to render outbound content"
        ))
    })?;

    Ok(CampaignContext {
        campaign_id,
        campaign_name: campaign["name"].as_str().unwrap_or("campaign").into(),
        channel: campaign["channel"].as_str().unwrap_or("email").into(),
        account_id,
        account_name: account["name"].as_str().unwrap_or("account").into(),
        account_domain: account["domain"].as_str().unwrap_or("").into(),
        contact_id,
        contact_email: contact["email"].as_str().unwrap_or("").into(),
        contact_phone_e164: contact["phone_e164"].as_str().map(ToOwned::to_owned),
        sequence_id,
        template_ref: family["template_ref"]
            .as_str()
            .unwrap_or("templates/default_v1.md")
            .into(),
        message_family_id: family["id"]
            .as_str()
            .and_then(|raw| Uuid::parse_str(raw).ok()),
    })
}

fn render_message(context: &CampaignContext) -> RenderedMessage {
    if context.channel == "whatsapp" {
        return RenderedMessage {
            subject: None,
            text_body: format!(
                "Hi from Minilab. Saw {} in {} and thought our {} playbook might be relevant.",
                context.account_name, context.account_domain, context.campaign_name
            ),
            html_body: None,
        };
    }

    if context.template_ref.contains("pain_v1") {
        let subject = format!("Quick idea for {}", context.account_name);
        let text_body = format!(
            "Hi,\n\nI noticed {} in {}. We built {} to help teams like yours move faster without losing control.\n\nIf useful, I can send a short breakdown.\n",
            context.account_name, context.account_domain, context.campaign_name
        );
        let html_body = format!(
            "<p>Hi,</p><p>I noticed <strong>{}</strong> in {}. We built <strong>{}</strong> to help teams like yours move faster without losing control.</p><p>If useful, I can send a short breakdown.</p>",
            context.account_name, context.account_domain, context.campaign_name
        );
        return RenderedMessage {
            subject: Some(subject),
            text_body,
            html_body: Some(html_body),
        };
    }

    let subject = format!("A note for {}", context.account_name);
    let text_body = format!(
        "Hi,\n\nReaching out about {} for {}.\n",
        context.campaign_name, context.account_name
    );
    let html_body = format!(
        "<p>Hi,</p><p>Reaching out about <strong>{}</strong> for {}.</p>",
        context.campaign_name, context.account_name
    );
    RenderedMessage {
        subject: Some(subject),
        text_body,
        html_body: Some(html_body),
    }
}

fn lowered_command(
    action_name: &str,
    params: Map<String, Value>,
    node_name: &str,
) -> Result<OperationalCommand, StoreError> {
    let action = CanonicalActionId::parse(action_name).map_err(|err| {
        StoreError::Contract(format!(
            "failed to parse outbound action `{action_name}` as canonical identity: {err}"
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
        .map_err(|err| StoreError::Contract(format!("failed to lower outbound command: {err}")))?;
    Ok(command)
}

fn dispatch_mock(
    context: &CampaignContext,
    rendered: &RenderedMessage,
    correlation_id: Uuid,
) -> Result<DeliveryReceipt, StoreError> {
    let mut params = Map::new();
    params.insert("channel".into(), json!(context.channel));
    params.insert("sequence_id".into(), json!(context.sequence_id));
    params.insert("correlation_id".into(), json!(correlation_id));
    params.insert("to".into(), json!(context.contact_email));
    params.insert("template_ref".into(), json!(context.template_ref));
    if let Some(subject) = &rendered.subject {
        params.insert("subject".into(), json!(subject));
    }
    params.insert("body".into(), json!(rendered.text_body));
    let operational_command =
        lowered_command("twilio.mock_outbound", params, "twilio-mock-outbound")?;

    Ok(DeliveryReceipt {
        provider: "mock",
        provider_message_id: format!("mock:{}", context.sequence_id),
        channel: context.channel.clone(),
        mock: true,
        operational_command,
        template_ref: context.template_ref.clone(),
        message_family_id: context.message_family_id,
    })
}

async fn dispatch_twilio_sendgrid(
    context: &CampaignContext,
    rendered: &RenderedMessage,
    correlation_id: Uuid,
) -> Result<DeliveryReceipt, StoreError> {
    let api_key = std::env::var("SENDGRID_API_KEY")
        .map_err(|_| StoreError::Env("SENDGRID_API_KEY".into()))?;
    let from_email = std::env::var("TWILIO_SENDGRID_FROM_EMAIL")
        .map_err(|_| StoreError::Env("TWILIO_SENDGRID_FROM_EMAIL".into()))?;
    let from_name = std::env::var("TWILIO_SENDGRID_FROM_NAME").ok();
    let base_url = std::env::var("TWILIO_SENDGRID_BASE_URL")
        .unwrap_or_else(|_| "https://api.sendgrid.com".into());
    let reply_to = build_sendgrid_reply_to(correlation_id);
    let subject = rendered
        .subject
        .clone()
        .ok_or_else(|| StoreError::Contract("email outbound requires a rendered subject".into()))?;

    let mut command_params = Map::new();
    command_params.insert("sequence_id".into(), json!(context.sequence_id));
    command_params.insert("campaign_id".into(), json!(context.campaign_id));
    command_params.insert("account_id".into(), json!(context.account_id));
    command_params.insert("contact_id".into(), json!(context.contact_id));
    command_params.insert("to_email".into(), json!(context.contact_email));
    command_params.insert("subject".into(), json!(subject));
    command_params.insert("template_ref".into(), json!(context.template_ref));
    let operational_command = lowered_command(
        "twilio.sendgrid_mail_send",
        command_params.clone(),
        "twilio-sendgrid-mail-send",
    )?;

    let mut body = json!({
        "personalizations": [{
            "to": [{ "email": context.contact_email }],
            "custom_args": {
                "sequence_id": context.sequence_id.to_string(),
                "correlation_id": correlation_id.to_string(),
                "campaign_id": context.campaign_id.to_string(),
            }
        }],
        "from": { "email": from_email },
        "subject": subject,
        "headers": {
            "X-Minilab-Correlation-Id": correlation_id.to_string(),
            "X-Minilab-Sequence-Id": context.sequence_id.to_string(),
        },
        "content": [{ "type": "text/plain", "value": rendered.text_body }],
    });
    if let Some(name) = from_name {
        body["from"]["name"] = json!(name);
    }
    if let Some(reply_to_email) = reply_to {
        body["reply_to"] = json!({ "email": reply_to_email });
    }
    if let Some(html) = &rendered.html_body {
        body["content"]
            .as_array_mut()
            .expect("content array")
            .push(json!({ "type": "text/html", "value": html }));
    }

    let response = reqwest::Client::new()
        .post(format!("{base_url}/v3/mail/send"))
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status().as_u16();
    if status != 202 {
        let text = response.text().await.unwrap_or_default();
        return Err(StoreError::Provider {
            provider: "twilio_sendgrid",
            status,
            body: text,
        });
    }

    let provider_message_id = response
        .headers()
        .get("x-message-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("accepted:{}", context.sequence_id));

    Ok(DeliveryReceipt {
        provider: "twilio_sendgrid",
        provider_message_id,
        channel: "email".into(),
        mock: false,
        operational_command,
        template_ref: context.template_ref.clone(),
        message_family_id: context.message_family_id,
    })
}

fn build_sendgrid_reply_to(correlation_id: Uuid) -> Option<String> {
    let domain = std::env::var("MINILAB_REPLY_EMAIL_DOMAIN").ok()?;
    let local_part =
        std::env::var("MINILAB_REPLY_EMAIL_LOCALPART").unwrap_or_else(|_| "reply".into());
    Some(format!("{local_part}+{correlation_id}@{domain}"))
}

async fn dispatch_twilio_whatsapp(
    context: &CampaignContext,
    rendered: &RenderedMessage,
    correlation_id: Uuid,
) -> Result<DeliveryReceipt, StoreError> {
    let account_sid = std::env::var("TWILIO_ACCOUNT_SID")
        .map_err(|_| StoreError::Env("TWILIO_ACCOUNT_SID".into()))?;
    let auth_token = std::env::var("TWILIO_AUTH_TOKEN")
        .map_err(|_| StoreError::Env("TWILIO_AUTH_TOKEN".into()))?;
    let from = std::env::var("TWILIO_WHATSAPP_FROM")
        .map_err(|_| StoreError::Env("TWILIO_WHATSAPP_FROM".into()))?;
    let to = context.contact_phone_e164.clone().ok_or_else(|| {
        StoreError::Contract(format!(
            "contact {} has no phone_e164 for WhatsApp delivery",
            context.contact_id
        ))
    })?;

    let mut command_params = Map::new();
    command_params.insert("sequence_id".into(), json!(context.sequence_id));
    command_params.insert("campaign_id".into(), json!(context.campaign_id));
    command_params.insert("account_id".into(), json!(context.account_id));
    command_params.insert("contact_id".into(), json!(context.contact_id));
    command_params.insert("to_whatsapp".into(), json!(to));
    command_params.insert("template_ref".into(), json!(context.template_ref));
    command_params.insert("correlation_id".into(), json!(correlation_id));
    let operational_command = lowered_command(
        "twilio.whatsapp_message",
        command_params,
        "twilio-whatsapp-message",
    )?;

    let response = reqwest::Client::new()
        .post(format!(
            "https://api.twilio.com/2010-04-01/Accounts/{account_sid}/Messages.json"
        ))
        .basic_auth(&account_sid, Some(&auth_token))
        .form(&[
            ("From", from),
            ("To", format!("whatsapp:{to}")),
            ("Body", rendered.text_body.clone()),
        ])
        .send()
        .await?;

    let status = response.status().as_u16();
    if status >= 300 {
        let text = response.text().await.unwrap_or_default();
        return Err(StoreError::Provider {
            provider: "twilio_whatsapp",
            status,
            body: text,
        });
    }

    let body = response.json::<Value>().await?;
    let provider_message_id = body["sid"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("accepted:{}", context.sequence_id));

    Ok(DeliveryReceipt {
        provider: "twilio_whatsapp",
        provider_message_id,
        channel: "whatsapp".into(),
        mock: false,
        operational_command,
        template_ref: context.template_ref.clone(),
        message_family_id: context.message_family_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_sendgrid_reply_to, effective_provider, lowered_command, render_message,
        CampaignContext, OutboundProvider,
    };
    use minilab_core::SimMode;
    use serde_json::{json, Map};
    use uuid::Uuid;

    use crate::StoreClient;

    fn client(sim_mode: SimMode) -> StoreClient {
        StoreClient::with_mode("https://example.supabase.co", "service-key", sim_mode)
    }

    fn context(channel: &str, template_ref: &str) -> CampaignContext {
        CampaignContext {
            campaign_id: Uuid::nil(),
            campaign_name: "Outbound wedge".into(),
            channel: channel.into(),
            account_id: Uuid::nil(),
            account_name: "Acme".into(),
            account_domain: "acme.example".into(),
            contact_id: Uuid::nil(),
            contact_email: "buyer@acme.example".into(),
            contact_phone_e164: Some("+15551234567".into()),
            sequence_id: Uuid::nil(),
            template_ref: template_ref.into(),
            message_family_id: None,
        }
    }

    #[test]
    fn auto_provider_maps_email_to_sendgrid() {
        let resolved = effective_provider(
            OutboundProvider::Auto,
            "email",
            &client(SimMode::Production),
        )
        .unwrap();
        assert_eq!(resolved, OutboundProvider::TwilioSendGrid);
    }

    #[test]
    fn auto_provider_maps_whatsapp() {
        let resolved = effective_provider(
            OutboundProvider::Auto,
            "whatsapp",
            &client(SimMode::Production),
        )
        .unwrap();
        assert_eq!(resolved, OutboundProvider::TwilioWhatsApp);
    }

    #[test]
    fn non_production_auto_falls_back_to_mock() {
        let resolved = effective_provider(
            OutboundProvider::Auto,
            "email",
            &client(SimMode::Simulation),
        )
        .unwrap();
        assert_eq!(resolved, OutboundProvider::Mock);
    }

    #[test]
    fn rendered_email_contains_subject() {
        let rendered = render_message(&context("email", "templates/pain_v1.md"));
        assert!(rendered.subject.unwrap().contains("Acme"));
        assert!(rendered.html_body.unwrap().contains("Acme"));
    }

    #[test]
    fn rendered_whatsapp_has_no_subject() {
        let rendered = render_message(&context("whatsapp", "templates/wa_v1.txt"));
        assert!(rendered.subject.is_none());
        assert!(rendered.text_body.contains("Acme"));
    }

    #[test]
    fn lowerer_materializes_twilio_command() {
        let mut params = Map::new();
        params.insert("sequence_id".into(), json!(Uuid::nil()));
        let command = lowered_command("twilio.sendgrid_mail_send", params, "n1").unwrap();
        assert_eq!(command.namespace, "twilio");
        assert_eq!(command.verb, "sendgrid_mail_send");
    }

    #[test]
    fn reply_to_alias_uses_configured_domain() {
        std::env::set_var("MINILAB_REPLY_EMAIL_DOMAIN", "replies.minilab.example");
        std::env::remove_var("MINILAB_REPLY_EMAIL_LOCALPART");
        let reply_to = build_sendgrid_reply_to(Uuid::nil()).unwrap();
        assert_eq!(
            reply_to,
            "reply+00000000-0000-0000-0000-000000000000@replies.minilab.example"
        );
        std::env::remove_var("MINILAB_REPLY_EMAIL_DOMAIN");
    }
}
