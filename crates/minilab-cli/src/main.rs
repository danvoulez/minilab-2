//! Minilab CLI: the first executable body of the institution.
//!
//! The commands here intentionally start as local, receipt-producing probes.
//! They do not claim Cloudflare, SDKs, or external services as authority; they
//! transform inputs into nine-slot acts, validate/digest them, and leave local
//! evidence that can be replayed by later runtime slices.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

const NINE_SLOTS: [&str; 9] = [
    "who",
    "did",
    "this",
    "when",
    "confirmed_by",
    "if_ok",
    "if_doubt",
    "if_not",
    "status",
];

#[derive(Debug, Error)]
enum CliError {
    #[error("usage: {0}")]
    Usage(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid act: {0}")]
    InvalidAct(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NineSlotAct {
    who: String,
    did: String,
    this: Value,
    when: String,
    confirmed_by: Value,
    if_ok: String,
    if_doubt: String,
    if_not: String,
    status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Receipt {
    receipt_id: String,
    emitted_at: DateTime<Utc>,
    act: NineSlotAct,
    result: Value,
    evidence: Value,
    transport: Value,
    hashes: BTreeMap<String, String>,
}

impl NineSlotAct {
    fn planned(who: impl Into<String>, did: impl Into<String>, this: Value) -> Self {
        Self {
            who: who.into(),
            did: did.into(),
            this,
            when: Utc::now().to_rfc3339(),
            confirmed_by: json!([]),
            if_ok: "emit scoped receipt and continue".into(),
            if_doubt: "preserve doubt trace; do not touch the world".into(),
            if_not: "reject with classified receipt".into(),
            status: "candidate".into(),
        }
    }

    fn validate(&self) -> Result<(), CliError> {
        for (slot, value) in [
            ("who", self.who.as_str()),
            ("did", self.did.as_str()),
            ("when", self.when.as_str()),
            ("if_ok", self.if_ok.as_str()),
            ("if_doubt", self.if_doubt.as_str()),
            ("if_not", self.if_not.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(CliError::InvalidAct(format!("{slot} must be non-empty")));
            }
        }
        Ok(())
    }
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("minilab: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    let Some(domain) = args.first().map(String::as_str) else {
        return help();
    };
    match domain {
        "help" | "--help" | "-h" => help(),
        "logline" => logline(&args[1..]),
        "receipt" | "receipts" => receipts(&args[1..]),
        "workorder" => workorder(&args[1..]),
        "identity" => identity(&args[1..]),
        "alias" => alias(&args[1..]),
        "gate" => gate(&args[1..]),
        "admission" => admission(&args[1..]),
        "gateway" => gateway(&args[1..]),
        "lab" => lab(&args[1..]),
        "expedition" => expedition(&args[1..]),
        "incident" => incident(&args[1..]),
        "maintenance" => maintenance(&args[1..]),
        "ghost" => ghost(&args[1..]),
        "audit" => audit(&args[1..]),
        "snapshot" => snapshot(&args[1..]),
        "restore" => restore(&args[1..]),
        "release" => release(&args[1..]),
        "package" => package(&args[1..]),
        "build" => build(&args[1..]),
        "milestone" => milestone(&args[1..]),
        "adr" => adr(&args[1..]),
        "registry" => registry(&args[1..]),
        _ => Err(CliError::Usage(format!(
            "unknown domain {domain:?}; run `minilab help`"
        ))),
    }
}

fn help() -> Result<(), CliError> {
    println!(
        "Minilab CLI — Rust-native LogLine institution\n\n\
         Core:\n\
           minilab logline compile <file>\n\
           minilab logline walk <file>\n\
           minilab logline digest <file>\n\
           minilab receipt emit <file> [--out dir]\n\
           minilab receipt validate <receipt.json>\n\
           minilab workorder render-prompt <file>\n\n\
         Automation map probes are available for identity, gate, admission, gateway, lab,\n\
         expedition, incident, maintenance, ghost, audit, snapshot, restore, release,\n\
         package, build, milestone, adr, and registry."
    );
    Ok(())
}

fn logline(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("compile") => {
            let act = read_act(required(args, 1, "logline compile <file>")?)?;
            act.validate()?;
            print_json(&json!({"kind":"compiled_logline_act","slots":NINE_SLOTS,"act":act}))
        }
        Some("walk") => {
            let act = read_act(required(args, 1, "logline walk <file>")?)?;
            let status = match act.validate() {
                Ok(()) => "admissible_candidate",
                Err(_) => "doubt",
            };
            print_json(
                &json!({"walk":"local_probe","status":status,"touches_world":false,"act":act}),
            )
        }
        Some("digest") => {
            let bytes = fs::read(required(args, 1, "logline digest <file>")?)?;
            print_json(
                &json!({"sha256":sha256_hex(&bytes),"policy":"canonical bytes v0 local probe"}),
            )
        }
        _ => Err(CliError::Usage(
            "logline <compile|walk|digest> <file>".into(),
        )),
    }
}

fn receipts(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("emit") => {
            let input = required(args, 1, "receipt emit <file> [--out dir]")?;
            let out_dir = flag_value(args, "--out").unwrap_or("receipts");
            let act = read_act(input)?;
            act.validate()?;
            let receipt = make_receipt(
                act,
                json!({"status":"closed_local"}),
                json!({"source":input}),
            );
            fs::create_dir_all(out_dir)?;
            let path = Path::new(out_dir).join(format!("{}.json", receipt.receipt_id));
            fs::write(&path, serde_json::to_vec_pretty(&receipt)?)?;
            print_json(&json!({"receipt_path":path,"receipt":receipt}))
        }
        Some("validate") => {
            let receipt: Receipt = serde_json::from_slice(&fs::read(required(
                args,
                1,
                "receipt validate <receipt.json>",
            )?)?)?;
            let expected = receipt_hash(&receipt)?;
            let actual = receipt
                .hashes
                .get("receipt_without_hashes_sha256")
                .cloned()
                .unwrap_or_default();
            print_json(&json!({"valid": expected == actual,"expected":expected,"actual":actual}))
        }
        Some("index") => simple_probe("receipts.index", args.get(1).cloned()),
        _ => Err(CliError::Usage("receipt <emit|validate|index> ...".into())),
    }
}

fn workorder(args: &[String]) -> Result<(), CliError> {
    match args.first().map(String::as_str) {
        Some("render-prompt") => {
            let act = read_act(required(args, 1, "workorder render-prompt <file>")?)?;
            println!("You are operating Minilab. Preserve the nine slots, classify doubt, and produce receipts.\n\nAct:\n{}", serde_json::to_string_pretty(&act)?);
            Ok(())
        }
        Some("probe") | Some("to-logline") => {
            simple_probe(&format!("workorder.{}", args[0]), args.get(1).cloned())
        }
        _ => Err(CliError::Usage(
            "workorder <render-prompt|probe|to-logline> ...".into(),
        )),
    }
}

fn identity(args: &[String]) -> Result<(), CliError> {
    sub_probe("identity", args)
}
fn alias(args: &[String]) -> Result<(), CliError> {
    sub_probe("alias", args)
}
fn gate(args: &[String]) -> Result<(), CliError> {
    sub_probe("gate", args)
}
fn admission(args: &[String]) -> Result<(), CliError> {
    sub_probe("admission", args)
}
fn gateway(args: &[String]) -> Result<(), CliError> {
    sub_probe("gateway", args)
}
fn lab(args: &[String]) -> Result<(), CliError> {
    sub_probe("lab", args)
}
fn expedition(args: &[String]) -> Result<(), CliError> {
    sub_probe("expedition", args)
}
fn incident(args: &[String]) -> Result<(), CliError> {
    sub_probe("incident", args)
}
fn maintenance(args: &[String]) -> Result<(), CliError> {
    sub_probe("maintenance", args)
}
fn ghost(args: &[String]) -> Result<(), CliError> {
    sub_probe("ghost", args)
}
fn audit(args: &[String]) -> Result<(), CliError> {
    sub_probe("audit", args)
}
fn snapshot(args: &[String]) -> Result<(), CliError> {
    sub_probe("snapshot", args)
}
fn restore(args: &[String]) -> Result<(), CliError> {
    sub_probe("restore", args)
}
fn release(args: &[String]) -> Result<(), CliError> {
    sub_probe("release", args)
}
fn package(args: &[String]) -> Result<(), CliError> {
    sub_probe("package", args)
}
fn build(args: &[String]) -> Result<(), CliError> {
    sub_probe("build", args)
}
fn milestone(args: &[String]) -> Result<(), CliError> {
    sub_probe("milestone", args)
}
fn adr(args: &[String]) -> Result<(), CliError> {
    sub_probe("adr", args)
}
fn registry(args: &[String]) -> Result<(), CliError> {
    sub_probe("registry", args)
}

fn sub_probe(domain: &str, args: &[String]) -> Result<(), CliError> {
    let op = args.first().cloned().unwrap_or_else(|| "status".into());
    let subject = args.get(1).cloned();
    simple_probe(&format!("{domain}.{op}"), subject)
}

fn simple_probe(did: &str, subject: Option<String>) -> Result<(), CliError> {
    let act = NineSlotAct::planned(
        env::var("MINILAB_WHO").unwrap_or_else(|_| "local.operator".into()),
        did,
        json!({"subject": subject, "mode":"local_probe"}),
    );
    let receipt = make_receipt(
        act,
        json!({"status":"planned_probe","touches_world":false}),
        json!({"cli":"minilab","adapter":"none"}),
    );
    print_json(&receipt)
}

fn read_act(path: &str) -> Result<NineSlotAct, CliError> {
    let bytes = fs::read(path)?;
    if path.ends_with(".json") {
        return Ok(serde_json::from_slice(&bytes)?);
    }
    parse_logline_text(
        std::str::from_utf8(&bytes).map_err(|e| CliError::InvalidAct(e.to_string()))?,
    )
}

fn parse_logline_text(text: &str) -> Result<NineSlotAct, CliError> {
    let mut map = Map::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key == "this" || key == "confirmed_by" {
            map.insert(
                key.into(),
                serde_json::from_str(value).unwrap_or_else(|_| json!(value)),
            );
        } else {
            map.insert(key.into(), json!(value));
        }
    }
    for slot in NINE_SLOTS {
        if !map.contains_key(slot) {
            return Err(CliError::InvalidAct(format!(
                "missing required slot {slot}"
            )));
        }
    }
    Ok(serde_json::from_value(Value::Object(map))?)
}

fn make_receipt(act: NineSlotAct, result: Value, evidence: Value) -> Receipt {
    let mut receipt = Receipt {
        receipt_id: format!("rcpt_{}", Uuid::new_v4()),
        emitted_at: Utc::now(),
        act,
        result,
        evidence,
        transport: json!({"kind":"local_cli","authority":"rust_native_minilab"}),
        hashes: BTreeMap::new(),
    };
    let hash = receipt_hash(&receipt).unwrap_or_else(|_| "hash_error".into());
    receipt
        .hashes
        .insert("receipt_without_hashes_sha256".into(), hash);
    receipt
}

fn receipt_hash(receipt: &Receipt) -> Result<String, CliError> {
    let mut clone = receipt.clone();
    clone.hashes.clear();
    Ok(sha256_hex(&serde_json::to_vec(&clone)?))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn required<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, CliError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| CliError::Usage(usage.into()))
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nine_slot_logline_text() {
        let act = parse_logline_text(
            r#"
            who = dan
            did = logline.walk
            this = {"file":"demo.logline"}
            when = 2026-05-11T00:00:00Z
            confirmed_by = []
            if_ok = continue
            if_doubt = simulate
            if_not = reject
            status = candidate
            "#,
        )
        .unwrap();
        assert_eq!(act.who, "dan");
        assert_eq!(act.this["file"], "demo.logline");
    }

    #[test]
    fn receipt_hash_validates_after_round_trip() {
        let act = NineSlotAct::planned("dan", "receipt.emit", json!({"x":1}));
        let receipt = make_receipt(act, json!({"ok":true}), json!({"source":"test"}));
        assert_eq!(
            receipt.hashes.get("receipt_without_hashes_sha256").unwrap(),
            &receipt_hash(&receipt).unwrap()
        );
    }
}
