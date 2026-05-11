use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::client::StoreError;

type HmacSha1 = Hmac<Sha1>;

pub fn validate_twilio_signature(
    auth_token: &str,
    url: &str,
    form_params: &BTreeMap<String, String>,
    signature_header: &str,
) -> Result<bool, StoreError> {
    let mut data = url.to_string();
    for (key, value) in form_params {
        data.push_str(key);
        data.push_str(value);
    }

    let mut mac = HmacSha1::new_from_slice(auth_token.as_bytes())
        .map_err(|err| StoreError::Contract(format!("invalid twilio auth token: {err}")))?;
    mac.update(data.as_bytes());
    let expected = STANDARD.encode(mac.finalize().into_bytes());
    Ok(expected == signature_header)
}

pub fn validate_sendgrid_signature(
    public_key_b64: &str,
    timestamp: &str,
    raw_body: &[u8],
    signature_b64: &str,
) -> Result<bool, StoreError> {
    let key_bytes = STANDARD.decode(public_key_b64).map_err(|err| {
        StoreError::Contract(format!("invalid SendGrid public key (base64): {err}"))
    })?;
    let verifying_key = VerifyingKey::from_public_key_der(&key_bytes)
        .map_err(|err| StoreError::Contract(format!("invalid SendGrid public key (DER): {err}")))?;
    let signature_bytes = STANDARD.decode(signature_b64).map_err(|err| {
        StoreError::Contract(format!("invalid SendGrid signature (base64): {err}"))
    })?;
    let signature = Signature::from_der(&signature_bytes)
        .map_err(|err| StoreError::Contract(format!("invalid SendGrid signature (DER): {err}")))?;

    let mut hasher = Sha256::new();
    hasher.update(timestamp.as_bytes());
    hasher.update(raw_body);
    let digest = hasher.finalize();

    Ok(verifying_key.verify(&digest, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::{validate_sendgrid_signature, validate_twilio_signature};
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use hmac::{Hmac, Mac};
    use p256::ecdsa::{signature::Signer, SigningKey};
    use p256::pkcs8::EncodePublicKey;
    use sha1::Sha1;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    type HmacSha1 = Hmac<Sha1>;

    #[test]
    fn twilio_signature_validation_accepts_matching_signature() {
        let url = "https://example.com/twilio/inbound";
        let params = BTreeMap::from([
            ("Body".to_string(), "hello".to_string()),
            ("From".to_string(), "whatsapp:+15551234567".to_string()),
        ]);

        let mut mac = HmacSha1::new_from_slice(b"auth-token").unwrap();
        mac.update(b"https://example.com/twilio/inboundBodyhelloFromwhatsapp:+15551234567");
        let signature = STANDARD.encode(mac.finalize().into_bytes());

        assert!(validate_twilio_signature("auth-token", url, &params, &signature).unwrap());
    }

    #[test]
    fn sendgrid_signature_validation_accepts_matching_signature() {
        let signing_key = SigningKey::from_bytes((&[7u8; 32]).into()).unwrap();
        let verifying_key = signing_key.verifying_key();
        let public_der = verifying_key.to_public_key_der().unwrap();
        let body = br#"{"hello":"world"}"#;
        let timestamp = "1700000000";
        let mut digest_input = Vec::new();
        digest_input.extend_from_slice(timestamp.as_bytes());
        digest_input.extend_from_slice(body);
        let digest = Sha256::digest(&digest_input);
        let signature: p256::ecdsa::Signature = signing_key.sign(&digest);
        let signature_b64 = STANDARD.encode(signature.to_der().as_bytes());
        let public_key_b64 = STANDARD.encode(public_der.as_ref());

        assert!(
            validate_sendgrid_signature(&public_key_b64, timestamp, body, &signature_b64).unwrap()
        );
    }
}
