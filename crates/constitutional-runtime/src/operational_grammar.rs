//! Operational grammar: the runtime's emission surface.
//!
//! Per the Minilab Formal Spec and the operational grammar ADR (extracted to
//! `extracted/key/02-operational-grammar.md` / `07-ir-to-operational-mapping.md`),
//! the runtime lives on a **tiny, flat, deterministic** line-oriented language:
//!
//! ```text
//! namespace.verb key=value key="quoted value" ...
//! ```
//!
//! Programs are sequences of such lines, optionally with indented children
//! forming a single-level block:
//!
//! ```text
//! flow.verify_report target=lab8gb infer=lab512
//!   emit thread=t1
//!   confirm role=admin
//! ```
//!
//! The language is deliberately **boring**:
//!
//! - no expressions, no types, no string interpolation,
//! - one pass, one position cursor, one closed error set,
//! - equal inputs parse to equal [`OperationalProgram`] values byte-for-byte,
//! - the surface never grows — new IR primitives lower to new *verbs*, not
//!   new *syntax*.
//!
//! This module is the **canonical parser** for that surface and the bridge
//! to the constitutional IR. It is the stable compiler boundary referenced
//! by the Manual do Transplante, Phase 2.
//!
//! ## Layers
//!
//! 1. `parse_program` / `parse_line` — surface text → AST.
//! 2. `OperationalProgram::normalize` — structural well-formedness.
//! 3. `OperationalLine::to_ir_primitive` — closed mapping AST → [`IRPrimitive`]
//!    for the subset of verbs with unambiguous IR shape today.
//!
//! Verbs outside the closed subset are legal operational lines but do **not**
//! lower to IR here — they are accepted at parse time and rejected (with a
//! structured reason) at lowering time. That is deliberate: the surface is
//! open to vocabulary growth, but the IR bridge is a constitutional act.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};

use crate::act_identity::CanonicalActionId;
use crate::ir::{
    ActionKind, IRPrimitive, InferSurface, Kind, ReconcileMode, Role, Schema, Trigger, Window,
};
use crate::refs::{DataRef, PolicyId, SurfaceRef, TargetRef};

// -------------------------------------------------------------------------
// AST
// -------------------------------------------------------------------------

/// A parsed argument value. We preserve whether the surface used quoting,
/// because it is the only way to carry values containing whitespace, `=`,
/// or a leading digit through the grammar unambiguously. Downstream code
/// may treat `Bare` and `Quoted` uniformly when the string content is all
/// it cares about.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", content = "value", rename_all = "snake_case")]
pub enum ArgValue {
    Bare(String),
    Quoted(String),
}

impl ArgValue {
    pub fn as_str(&self) -> &str {
        match self {
            ArgValue::Bare(s) | ArgValue::Quoted(s) => s.as_str(),
        }
    }

    pub fn into_string(self) -> String {
        match self {
            ArgValue::Bare(s) | ArgValue::Quoted(s) => s,
        }
    }
}

impl fmt::Display for ArgValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgValue::Bare(s) => f.write_str(s),
            ArgValue::Quoted(s) => write!(f, "\"{}\"", s.replace('"', "\\\"")),
        }
    }
}

/// A single operational line: `namespace.verb key=value ...`.
///
/// Args are stored in a `BTreeMap` so iteration / serialization is stable —
/// replay determinism is an invariant of the whole runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationalLine {
    pub namespace: String,
    pub verb: String,
    pub args: BTreeMap<String, ArgValue>,
}

impl OperationalLine {
    pub fn kind(&self) -> String {
        format!("{}.{}", self.namespace, self.verb)
    }

    pub fn arg(&self, key: &str) -> Option<&ArgValue> {
        self.args.get(key)
    }

    pub fn arg_str(&self, key: &str) -> Option<&str> {
        self.arg(key).map(ArgValue::as_str)
    }

    pub fn require(&self, key: &str) -> Result<&str, IrLoweringError> {
        self.arg_str(key)
            .ok_or_else(|| IrLoweringError::MissingArg {
                kind: self.kind(),
                arg: key.to_string(),
            })
    }
}

impl fmt::Display for OperationalLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace, self.verb)?;
        for (k, v) in &self.args {
            write!(f, " {}={}", k, v)?;
        }
        Ok(())
    }
}

/// A line plus its (optional) child block. The grammar allows exactly one
/// level of indentation today; nesting beyond that is a structural grammar
/// concern, not operational.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationalEntry {
    pub line: OperationalLine,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OperationalEntry>,
}

impl OperationalEntry {
    pub fn leaf(line: OperationalLine) -> Self {
        Self {
            line,
            children: Vec::new(),
        }
    }
}

/// A full operational program: a flat sequence of top-level entries, each of
/// which may carry an indented child block.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationalProgram {
    pub entries: Vec<OperationalEntry>,
}

impl OperationalProgram {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Structural well-formedness checks beyond what the parser already
    /// enforces. Idempotent; returns `Ok(())` if the program is clean.
    ///
    /// Today this checks:
    ///
    /// - no top-level entry has an empty namespace or verb,
    /// - children respect the same rule recursively.
    ///
    /// Future rules (capability hints, reserved verb namespaces) will live
    /// here so the parser stays a pure recognizer.
    pub fn normalize(&self) -> Result<(), ParseError> {
        fn walk(e: &OperationalEntry) -> Result<(), ParseError> {
            if e.line.namespace.is_empty() || e.line.verb.is_empty() {
                return Err(ParseError {
                    line: 0,
                    col: 0,
                    kind: ParseErrorKind::EmptyKind,
                });
            }
            for c in &e.children {
                walk(c)?;
            }
            Ok(())
        }
        for e in &self.entries {
            walk(e)?;
        }
        Ok(())
    }
}

// -------------------------------------------------------------------------
// Parser errors
// -------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    /// 1-indexed line in the input (before stripping comments / blanks).
    pub line: usize,
    /// 1-indexed column in the source line.
    pub col: usize,
    pub kind: ParseErrorKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// Reached end of input where another token was required.
    UnexpectedEnd,
    /// Character not permitted in the current production.
    UnexpectedChar(char),
    /// `namespace.verb` head is missing one side.
    MalformedKind,
    /// Missing `.` between namespace and verb.
    KindMissingDot,
    /// Argument `=` separator missing.
    MissingEqualsInArg,
    /// Quoted literal never closed.
    UnterminatedQuote,
    /// A duplicate argument key on the same line.
    DuplicateArg(String),
    /// Indentation does not form a valid child block (e.g. first line is indented).
    UnexpectedIndent,
    /// Indentation width is inconsistent with the program's first indent.
    MixedIndentation,
    /// Normalization caught an empty kind after parse.
    EmptyKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} col {}: ", self.line, self.col)?;
        match &self.kind {
            ParseErrorKind::UnexpectedEnd => f.write_str("unexpected end of input"),
            ParseErrorKind::UnexpectedChar(c) => write!(f, "unexpected character {:?}", c),
            ParseErrorKind::MalformedKind => f.write_str("malformed namespace.verb head"),
            ParseErrorKind::KindMissingDot => {
                f.write_str("expected '.' between namespace and verb")
            }
            ParseErrorKind::MissingEqualsInArg => f.write_str("argument missing '='"),
            ParseErrorKind::UnterminatedQuote => f.write_str("unterminated quoted value"),
            ParseErrorKind::DuplicateArg(k) => write!(f, "duplicate argument {:?}", k),
            ParseErrorKind::UnexpectedIndent => f.write_str("unexpected indentation"),
            ParseErrorKind::MixedIndentation => f.write_str("mixed / inconsistent indentation"),
            ParseErrorKind::EmptyKind => f.write_str("empty namespace or verb"),
        }
    }
}

impl std::error::Error for ParseError {}

// -------------------------------------------------------------------------
// Parser
// -------------------------------------------------------------------------

/// Parse a single line (no indentation, no comments, no newlines). Prefer
/// [`parse_program`] for multi-line input.
pub fn parse_line(src: &str) -> Result<OperationalLine, ParseError> {
    let mut p = LineParser::new(src, 1);
    let l = p.parse()?;
    p.expect_end()?;
    Ok(l)
}

/// Parse a full program. Comment lines (`#` prefix after stripping whitespace)
/// and blank lines are ignored. Indentation is significant: an indented line
/// is a child of the previous top-level line. Only one level of nesting is
/// supported.
pub fn parse_program(src: &str) -> Result<OperationalProgram, ParseError> {
    let mut entries: Vec<OperationalEntry> = Vec::new();
    let mut indent_unit: Option<&str> = None;

    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;

        // Skip blank and comment-only lines.
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }

        let indent_len = raw.len() - trimmed.len();
        let indent = &raw[..indent_len];

        if indent.is_empty() {
            // Top-level entry.
            let mut lp = LineParser::new(trimmed, line_no);
            let line = lp.parse()?;
            lp.expect_end()?;
            entries.push(OperationalEntry::leaf(line));
        } else {
            // Child entry.
            if entries.is_empty() {
                return Err(ParseError {
                    line: line_no,
                    col: 1,
                    kind: ParseErrorKind::UnexpectedIndent,
                });
            }
            match indent_unit {
                None => indent_unit = leak_indent_unit(indent),
                Some(u) if u == indent => {}
                Some(_) => {
                    return Err(ParseError {
                        line: line_no,
                        col: 1,
                        kind: ParseErrorKind::MixedIndentation,
                    });
                }
            }
            let mut lp = LineParser::new(trimmed, line_no);
            let line = lp.parse()?;
            lp.expect_end()?;
            let parent = entries.last_mut().expect("non-empty by branch");
            parent.children.push(OperationalEntry::leaf(line));
        }
    }

    Ok(OperationalProgram { entries })
}

/// Helper: coerce a transient `&str` borrow into a `'static` reference by
/// copying to a box. Used because `indent_unit` must outlive individual
/// source lines but we don't want to clone on every check.
///
/// This is a closed, bounded leak (a handful of bytes) that happens at most
/// once per parsed program.
fn leak_indent_unit(s: &str) -> Option<&'static str> {
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    Some(leaked)
}

/// Line-level parser. Stateful over a single source line.
struct LineParser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line_no: usize,
}

impl<'a> LineParser<'a> {
    fn new(src: &'a str, line_no: usize) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line_no,
        }
    }

    fn col(&self) -> usize {
        self.pos + 1
    }

    fn err(&self, kind: ParseErrorKind) -> ParseError {
        ParseError {
            line: self.line_no,
            col: self.col(),
            kind,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn eat_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.pos += 1;
        }
    }

    fn expect_end(&mut self) -> Result<(), ParseError> {
        self.eat_spaces();
        match self.peek() {
            None => Ok(()),
            Some(c) => Err(self.err(ParseErrorKind::UnexpectedChar(c as char))),
        }
    }

    fn parse(&mut self) -> Result<OperationalLine, ParseError> {
        self.eat_spaces();
        let namespace = self.parse_ident()?;
        match self.peek() {
            Some(b'.') => {
                self.bump();
            }
            Some(_) | None => return Err(self.err(ParseErrorKind::KindMissingDot)),
        }
        let verb = self.parse_ident()?;
        if namespace.is_empty() || verb.is_empty() {
            return Err(self.err(ParseErrorKind::MalformedKind));
        }

        let mut args: BTreeMap<String, ArgValue> = BTreeMap::new();
        loop {
            self.eat_spaces();
            if self.peek().is_none() {
                break;
            }
            let key = self.parse_ident()?;
            if key.is_empty() {
                return Err(self.err(ParseErrorKind::MalformedKind));
            }
            match self.peek() {
                Some(b'=') => {
                    self.bump();
                }
                _ => return Err(self.err(ParseErrorKind::MissingEqualsInArg)),
            }
            let value = self.parse_value()?;
            if args.insert(key.clone(), value).is_some() {
                return Err(self.err(ParseErrorKind::DuplicateArg(key)));
            }
        }

        Ok(OperationalLine {
            namespace,
            verb,
            args,
        })
    }

    fn parse_ident(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        if !matches!(self.peek(), Some(c) if is_ident_start(c)) {
            match self.peek() {
                Some(c) => return Err(self.err(ParseErrorKind::UnexpectedChar(c as char))),
                None => return Err(self.err(ParseErrorKind::UnexpectedEnd)),
            }
        }
        self.bump();
        while let Some(c) = self.peek() {
            if is_ident_cont(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn parse_value(&mut self) -> Result<ArgValue, ParseError> {
        match self.peek() {
            Some(b'"') => self.parse_quoted(),
            Some(_) => self.parse_bare(),
            None => Err(self.err(ParseErrorKind::UnexpectedEnd)),
        }
    }

    fn parse_bare(&mut self) -> Result<ArgValue, ParseError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.err(ParseErrorKind::UnexpectedEnd));
        }
        Ok(ArgValue::Bare(self.src[start..self.pos].to_string()))
    }

    fn parse_quoted(&mut self) -> Result<ArgValue, ParseError> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.bump(); // consume opening quote
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(ParseErrorKind::UnterminatedQuote)),
                Some(b'"') => return Ok(ArgValue::Quoted(out)),
                Some(b'\\') => match self.bump() {
                    None => return Err(self.err(ParseErrorKind::UnterminatedQuote)),
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'n') => out.push('\n'),
                    Some(b't') => out.push('\t'),
                    Some(other) => out.push(other as char),
                },
                Some(c) => out.push(c as char),
            }
        }
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

// -------------------------------------------------------------------------
// AST → IR (closed subset)
// -------------------------------------------------------------------------

/// Errors that can occur when lowering an operational line to an IR primitive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IrLoweringError {
    /// The `namespace.verb` pair has no IR mapping today.
    UnmappedVerb { kind: String },
    /// A required argument was absent.
    MissingArg { kind: String, arg: String },
    /// An argument had an unrecognized enumerated value.
    InvalidEnumValue {
        kind: String,
        arg: String,
        value: String,
        allowed: &'static [&'static str],
    },
    /// PR 5b.1: the action identity supplied on the surface could not be
    /// parsed as a [`CanonicalActionId`]. Surfaces the canonical-identity
    /// boundary without dispatching guessed strings.
    InvalidActionIdentity {
        kind: String,
        arg: String,
        value: String,
    },
}

impl fmt::Display for IrLoweringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrLoweringError::UnmappedVerb { kind } => {
                write!(f, "verb {:?} has no IR mapping", kind)
            }
            IrLoweringError::MissingArg { kind, arg } => {
                write!(f, "verb {:?} requires argument {:?}", kind, arg)
            }
            IrLoweringError::InvalidEnumValue {
                kind,
                arg,
                value,
                allowed,
            } => write!(
                f,
                "verb {:?} got {}={:?}, expected one of {:?}",
                kind, arg, value, allowed
            ),
            IrLoweringError::InvalidActionIdentity { kind, arg, value } => write!(
                f,
                "verb {:?} got {}={:?} which is not a valid canonical action identity (namespace.verb, lowercase/digits/underscore)",
                kind, arg, value
            ),
        }
    }
}

impl std::error::Error for IrLoweringError {}

impl OperationalLine {
    /// Lower this line into an [`IRPrimitive`]. Only the closed subset
    /// documented in `extracted/key/07-ir-to-operational-mapping.md` is
    /// supported; other verbs return [`IrLoweringError::UnmappedVerb`]. This
    /// is deliberate — new verbs become new IR bridges through an ADR, not
    /// through silent defaults.
    pub fn to_ir_primitive(&self) -> Result<IRPrimitive, IrLoweringError> {
        let kind = self.kind();
        match (self.namespace.as_str(), self.verb.as_str()) {
            // OBSERVE family
            ("host", "inspect") => Ok(IRPrimitive::Observe {
                target: TargetRef(self.require("target")?.to_string()),
                scope: self.arg_str("scope").unwrap_or("default").to_string(),
            }),
            ("host", "facts") => Ok(IRPrimitive::Observe {
                target: TargetRef(self.require("target")?.to_string()),
                scope: "facts".to_string(),
            }),

            // COLLECT
            ("lab", "collect") => Ok(IRPrimitive::Collect {
                kind: Kind(self.require("kind")?.to_string()),
                target: TargetRef(self.require("target")?.to_string()),
                window: Window(self.require("window")?.to_string()),
            }),

            // COMPRESS
            ("lab", "compress") => Ok(IRPrimitive::Compress {
                kind: Kind(self.require("kind")?.to_string()),
                input_ref: DataRef(self.require("target")?.to_string()),
                infer_surface: parse_infer_surface(
                    &kind,
                    self.arg_str("infer").unwrap_or("local"),
                )?,
            }),

            // CLASSIFY
            ("lab", "classify") => Ok(IRPrimitive::Classify {
                kind: Kind(self.require("kind")?.to_string()),
                input_ref: DataRef(self.require("target")?.to_string()),
                schema: Schema(self.arg_str("schema").unwrap_or("default").to_string()),
                infer_surface: parse_infer_surface(
                    &kind,
                    self.arg_str("infer").unwrap_or("local"),
                )?,
            }),

            // PRIORITIZE
            ("lab", "prioritize") => Ok(IRPrimitive::Prioritize {
                kind: Kind(self.require("kind")?.to_string()),
                input_ref: DataRef(self.require("target")?.to_string()),
                policy: PolicyId(self.arg_str("policy").unwrap_or("default").to_string()),
                infer_surface: parse_infer_surface(
                    &kind,
                    self.arg_str("infer").unwrap_or("local"),
                )?,
            }),

            // SUMMARY / FLOW bridges
            //
            // These verbs remain operationally first-class, but they cross
            // the constitutional IR boundary as canonical `Execute` acts
            // instead of inventing new primitives outside the frozen set.
            ("lab", "summary") => {
                let _ = self.require("kind")?;
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                let mut params = action_params(self, &[]);
                params
                    .entry("target")
                    .or_insert_with(|| JsonValue::String("core".into()));
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(
                        CanonicalActionId::new("lab", "summary").unwrap(),
                    ),
                    params,
                })
            }
            ("lab", "drift") => {
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                let mut params = action_params(self, &[]);
                params
                    .entry("target")
                    .or_insert_with(|| JsonValue::String("core".into()));
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(CanonicalActionId::new("lab", "drift").unwrap()),
                    params,
                })
            }
            ("lab", "route") => {
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(CanonicalActionId::new("lab", "route").unwrap()),
                    params: action_params(self, &[]),
                })
            }
            ("lab", "organize") => {
                let _ = self.require("kind")?;
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                let mut params = action_params(self, &[]);
                params
                    .entry("target")
                    .or_insert_with(|| JsonValue::String("core".into()));
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(
                        CanonicalActionId::new("lab", "organize").unwrap(),
                    ),
                    params,
                })
            }
            ("flow", "verify_report") => {
                let _ = self.require("target")?;
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(
                        CanonicalActionId::new("flow", "verify_report").unwrap(),
                    ),
                    params: action_params(self, &[]),
                })
            }
            ("flow", "drift_review") => {
                let _ = self.require("target")?;
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(
                        CanonicalActionId::new("flow", "drift_review").unwrap(),
                    ),
                    params: action_params(self, &[]),
                })
            }
            ("flow", "recover_cmd") => {
                let _ = self.require("id")?;
                if let Some(infer) = self.arg_str("infer") {
                    let _ = parse_infer_surface(&kind, infer)?;
                }
                Ok(IRPrimitive::Execute {
                    action: ActionKind::Canonical(
                        CanonicalActionId::new("flow", "recover_cmd").unwrap(),
                    ),
                    params: action_params(self, &[]),
                })
            }

            // COMPARE
            ("lab", "compare") => Ok(IRPrimitive::Compare {
                kind: Kind(self.require("kind")?.to_string()),
                left: DataRef(self.arg_str("left").unwrap_or("desired").to_string()),
                right: DataRef(self.arg_str("right").unwrap_or("observed").to_string()),
            }),

            // RECONCILE
            ("host", "reconcile") => Ok(IRPrimitive::Reconcile {
                target: TargetRef(self.require("target")?.to_string()),
                desired: DataRef(self.arg_str("desired").unwrap_or("canonical").to_string()),
                mode: parse_reconcile_mode(&kind, self.arg_str("mode").unwrap_or("dry"))?,
            }),

            // EMIT
            ("chat", "reply") => Ok(IRPrimitive::Emit {
                surface: SurfaceRef(format!("thread:{}", self.require("thread")?)),
                payload: DataRef(self.arg_str("text").unwrap_or("").to_string()),
            }),

            // CONFIRM
            ("confirm", "request") => {
                let action_name = self.require("action")?.to_string();
                let action_id = CanonicalActionId::parse(&action_name).map_err(|_| {
                    IrLoweringError::InvalidActionIdentity {
                        kind: kind.clone(),
                        arg: "action".into(),
                        value: action_name.clone(),
                    }
                })?;
                let action = IRPrimitive::Execute {
                    action: ActionKind::Canonical(action_id),
                    params: action_params(self, &["kind", "action", "role"]),
                };
                Ok(IRPrimitive::Confirm {
                    action: Box::new(action),
                    role: Role(self.arg_str("role").unwrap_or("admin").to_string()),
                })
            }

            // SCHEDULE
            ("flow", "schedule") => {
                let kind_arg = self.require("kind")?.to_string();
                let flow_id = CanonicalActionId::new("flow", kind_arg.clone()).map_err(|_| {
                    IrLoweringError::InvalidActionIdentity {
                        kind: kind.clone(),
                        arg: "kind".into(),
                        value: kind_arg.clone(),
                    }
                })?;
                let inner = IRPrimitive::Execute {
                    action: ActionKind::Canonical(flow_id),
                    params: action_params(self, &["kind", "after", "at"]),
                };
                let trigger = self
                    .arg_str("after")
                    .or_else(|| self.arg_str("at"))
                    .unwrap_or("immediate")
                    .to_string();
                Ok(IRPrimitive::Schedule {
                    action: Box::new(inner),
                    trigger: Trigger(trigger),
                })
            }

            // CANCEL
            ("cmd", "cancel") | ("flow", "cancel") => Ok(IRPrimitive::Cancel {
                id: self.require("id")?.to_string(),
            }),

            // EXECUTE family — material acts lower to
            // `Execute { ActionKind::Canonical(...) }`. The identities are
            // constructed from known-valid literals so `.unwrap()` is
            // constitutional here. The closed set tracks the **landed
            // slices**, not a free-for-all: every verb in this block must
            // already have a live orchestrator wired into
            // `minilab-store::dispatcher`.
            ("host", "verify") => Ok(IRPrimitive::Execute {
                action: ActionKind::Canonical(CanonicalActionId::new("host", "verify").unwrap()),
                params: action_params(self, &[]),
            }),
            ("host", "pair") => Ok(IRPrimitive::Execute {
                action: ActionKind::Canonical(CanonicalActionId::new("host", "pair").unwrap()),
                params: action_params(self, &[]),
            }),
            ("outbound", "send") => Ok(IRPrimitive::Execute {
                action: ActionKind::Canonical(CanonicalActionId::new("outbound", "send").unwrap()),
                params: action_params(self, &[]),
            }),

            _ => Err(IrLoweringError::UnmappedVerb { kind }),
        }
    }
}

fn action_params(line: &OperationalLine, skip: &[&str]) -> Map<String, JsonValue> {
    let mut m = Map::new();
    for (k, v) in &line.args {
        if skip.contains(&k.as_str()) {
            continue;
        }
        m.insert(k.clone(), JsonValue::String(v.as_str().to_string()));
    }
    m
}

fn parse_infer_surface(kind: &str, v: &str) -> Result<InferSurface, IrLoweringError> {
    match v {
        "local" => Ok(InferSurface::Local),
        "lab256" => Ok(InferSurface::Lab256),
        "lab8gb" => Ok(InferSurface::Lab8gb),
        "lab512" => Ok(InferSurface::Lab512),
        "cloud" => Ok(InferSurface::Cloud),
        "hybrid" => Ok(InferSurface::Hybrid),
        other => Err(IrLoweringError::InvalidEnumValue {
            kind: kind.to_string(),
            arg: "infer".to_string(),
            value: other.to_string(),
            allowed: &["local", "cloud", "hybrid", "lab256", "lab8gb", "lab512"],
        }),
    }
}

fn parse_reconcile_mode(kind: &str, v: &str) -> Result<ReconcileMode, IrLoweringError> {
    match v {
        "apply" => Ok(ReconcileMode::Apply),
        "dry" | "dry_run" | "dry-run" => Ok(ReconcileMode::DryRun),
        "force" => Ok(ReconcileMode::Force),
        other => Err(IrLoweringError::InvalidEnumValue {
            kind: kind.to_string(),
            arg: "mode".to_string(),
            value: other.to_string(),
            allowed: &["apply", "dry", "force"],
        }),
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn line(src: &str) -> OperationalLine {
        parse_line(src).unwrap_or_else(|e| panic!("parse failed: {e}"))
    }

    #[test]
    fn parses_bare_kind_only() {
        let l = line("host.verify");
        assert_eq!(l.namespace, "host");
        assert_eq!(l.verb, "verify");
        assert!(l.args.is_empty());
    }

    #[test]
    fn parses_bare_kv_args() {
        let l = line("host.verify target=lab8gb");
        assert_eq!(l.args.len(), 1);
        assert_eq!(l.arg_str("target"), Some("lab8gb"));
    }

    #[test]
    fn parses_multiple_args_stable_order() {
        let l = line("lab.summary kind=drift target=core window=1h");
        // BTreeMap keys: kind, target, window (alphabetic), but Display
        // emits them alphabetically since we iterate BTreeMap directly.
        let rendered = format!("{}", l);
        assert_eq!(rendered, "lab.summary kind=drift target=core window=1h");
    }

    #[test]
    fn parses_quoted_value_with_spaces_and_equals() {
        let l = line(r#"chat.reply thread=t1 text="hello = world""#);
        assert_eq!(l.arg_str("text"), Some("hello = world"));
        match l.arg("text").unwrap() {
            ArgValue::Quoted(_) => {}
            other @ ArgValue::Bare(_) => panic!("expected quoted, got {other:?}"),
        }
    }

    #[test]
    fn parses_escaped_quotes_in_quoted_value() {
        let l = line(r#"chat.reply thread=t1 text="he said \"hi\"""#);
        assert_eq!(l.arg_str("text"), Some(r#"he said "hi""#));
    }

    #[test]
    fn rejects_missing_equals() {
        let err = parse_line("host.verify target").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::MissingEqualsInArg | ParseErrorKind::UnexpectedEnd
        ));
    }

    #[test]
    fn rejects_missing_dot() {
        let err = parse_line("hostverify").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::KindMissingDot);
    }

    #[test]
    fn rejects_unterminated_quote() {
        let err = parse_line(r#"chat.reply thread=t1 text="unterminated"#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnterminatedQuote);
    }

    #[test]
    fn rejects_duplicate_arg() {
        let err = parse_line("host.verify target=a target=b").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::DuplicateArg(ref k) if k == "target"));
    }

    #[test]
    fn parse_program_handles_blanks_and_comments() {
        let src = "\n# comment\nhost.verify target=a\n\n# another\nhost.verify target=b\n";
        let p = parse_program(src).unwrap();
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].line.arg_str("target"), Some("a"));
        assert_eq!(p.entries[1].line.arg_str("target"), Some("b"));
        p.normalize().unwrap();
    }

    #[test]
    fn parse_program_supports_indented_children() {
        let src = "flow.verify_report target=lab8gb\n  emit.to thread=t1\n  confirm.request action=host.reconcile role=admin\nhost.verify target=core\n";
        let p = parse_program(src).unwrap();
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].children.len(), 2);
        assert_eq!(p.entries[0].children[0].line.kind(), "emit.to");
        assert_eq!(p.entries[1].children.len(), 0);
    }

    #[test]
    fn parse_program_rejects_indent_before_any_parent() {
        let src = "  host.verify target=a\n";
        let err = parse_program(src).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnexpectedIndent);
    }

    #[test]
    fn parse_program_rejects_mixed_indentation() {
        let src = "host.verify target=a\n  host.verify target=b\n\thost.verify target=c\n";
        let err = parse_program(src).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::MixedIndentation);
    }

    #[test]
    fn normalize_is_idempotent_on_good_program() {
        let p = parse_program("host.verify target=a\nhost.verify target=b\n").unwrap();
        p.normalize().unwrap();
        p.normalize().unwrap();
    }

    // -- AST → IR lowering ------------------------------------------------

    #[test]
    fn lowers_host_inspect_to_observe() {
        let ir = line("host.inspect target=lab8gb scope=runtime")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Observe { target, scope } => {
                assert_eq!(target.0, "lab8gb");
                assert_eq!(scope, "runtime");
            }
            _ => panic!("expected Observe"),
        }
    }

    #[test]
    fn lowers_lab_collect_to_collect() {
        let ir = line("lab.collect kind=events target=core window=24h")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Collect {
                kind,
                target,
                window,
            } => {
                assert_eq!(kind.0, "events");
                assert_eq!(target.0, "core");
                assert_eq!(window.0, "24h");
            }
            _ => panic!("expected Collect"),
        }
    }

    #[test]
    fn lowers_lab_compress_with_named_infer_surface() {
        let ir = line("lab.compress kind=drift target=core infer=lab8gb")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Compress {
                kind,
                input_ref,
                infer_surface,
            } => {
                assert_eq!(kind.0, "drift");
                assert_eq!(input_ref.0, "core");
                assert_eq!(infer_surface, InferSurface::Lab8gb);
            }
            _ => panic!("expected Compress"),
        }
    }

    #[test]
    fn lowers_lab_classify_with_named_infer_surface() {
        let ir = line("lab.classify kind=failures target=core infer=lab512")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Classify {
                kind,
                input_ref,
                infer_surface,
                ..
            } => {
                assert_eq!(kind.0, "failures");
                assert_eq!(input_ref.0, "core");
                assert_eq!(infer_surface, InferSurface::Lab512);
            }
            _ => panic!("expected Classify"),
        }
    }

    #[test]
    fn lowers_lab_prioritize_with_lab256_infer_surface() {
        let ir = line("lab.prioritize kind=attention target=core infer=lab256")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Prioritize {
                kind,
                input_ref,
                infer_surface,
                ..
            } => {
                assert_eq!(kind.0, "attention");
                assert_eq!(input_ref.0, "core");
                assert_eq!(infer_surface, InferSurface::Lab256);
            }
            _ => panic!("expected Prioritize"),
        }
    }

    #[test]
    fn lowers_lab_summary_to_execute_bridge_preserving_infer_and_emit() {
        let ir = line("lab.summary kind=drift target=core infer=lab512 emit=thread:t42")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "lab.summary");
                assert_eq!(params.get("kind").and_then(|v| v.as_str()), Some("drift"));
                assert_eq!(params.get("target").and_then(|v| v.as_str()), Some("core"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t42")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_flow_verify_report_to_execute_bridge() {
        let ir = line("flow.verify_report target=lab8gb infer=lab512 emit=thread:t1")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "flow.verify_report");
                assert_eq!(
                    params.get("target").and_then(|v| v.as_str()),
                    Some("lab8gb")
                );
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t1")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_lab_drift_to_execute_bridge() {
        let ir = line("lab.drift target=core window=1h infer=lab512 emit=thread:t7")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "lab.drift");
                assert_eq!(params.get("target").and_then(|v| v.as_str()), Some("core"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t7")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_lab_route_to_execute_bridge() {
        let ir = line("lab.route window=1h infer=lab512 emit=thread:t9")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "lab.route");
                assert_eq!(params.get("window").and_then(|v| v.as_str()), Some("1h"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t9")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_lab_organize_to_execute_bridge() {
        let ir = line("lab.organize kind=attention target=core infer=lab512 emit=thread:t3")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "lab.organize");
                assert_eq!(
                    params.get("kind").and_then(|v| v.as_str()),
                    Some("attention")
                );
                assert_eq!(params.get("target").and_then(|v| v.as_str()), Some("core"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_flow_drift_review_to_execute_bridge() {
        let ir = line("flow.drift_review target=core window=1h infer=lab512 emit=thread:t2")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "flow.drift_review");
                assert_eq!(params.get("target").and_then(|v| v.as_str()), Some("core"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t2")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_flow_recover_cmd_to_execute_bridge() {
        let ir = line("flow.recover_cmd id=cmd-1 infer=lab512 emit=thread:t5")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Execute {
                action: ActionKind::Canonical(id),
                params,
            } => {
                assert_eq!(id.dotted_str(), "flow.recover_cmd");
                assert_eq!(params.get("id").and_then(|v| v.as_str()), Some("cmd-1"));
                assert_eq!(params.get("infer").and_then(|v| v.as_str()), Some("lab512"));
                assert_eq!(
                    params.get("emit").and_then(|v| v.as_str()),
                    Some("thread:t5")
                );
            }
            _ => panic!("expected Execute bridge"),
        }
    }

    #[test]
    fn lowers_host_reconcile_with_mode_parsing() {
        let apply = line("host.reconcile target=lab8gb mode=apply")
            .to_ir_primitive()
            .unwrap();
        assert!(matches!(
            apply,
            IRPrimitive::Reconcile {
                mode: ReconcileMode::Apply,
                ..
            }
        ));
        let dry = line("host.reconcile target=lab8gb mode=dry")
            .to_ir_primitive()
            .unwrap();
        assert!(matches!(
            dry,
            IRPrimitive::Reconcile {
                mode: ReconcileMode::DryRun,
                ..
            }
        ));
    }

    #[test]
    fn reconcile_rejects_bogus_mode_value() {
        let err = line("host.reconcile target=x mode=yolo")
            .to_ir_primitive()
            .unwrap_err();
        match err {
            IrLoweringError::InvalidEnumValue { arg, value, .. } => {
                assert_eq!(arg, "mode");
                assert_eq!(value, "yolo");
            }
            _ => panic!("expected InvalidEnumValue"),
        }
    }

    #[test]
    fn lowers_chat_reply_to_emit_with_thread_surface() {
        let ir = line(r#"chat.reply thread=t123 text="hi""#)
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Emit { surface, payload } => {
                assert_eq!(surface.0, "thread:t123");
                assert_eq!(payload.0, "hi");
            }
            _ => panic!("expected Emit"),
        }
    }

    #[test]
    fn lowers_confirm_request_to_confirm_wrapping_execute() {
        let ir =
            line("confirm.request kind=reconcile target=lab8gb action=host.reconcile role=admin")
                .to_ir_primitive()
                .unwrap();
        match ir {
            IRPrimitive::Confirm { action, role } => {
                assert_eq!(role.0, "admin");
                match *action {
                    IRPrimitive::Execute {
                        action: ActionKind::Canonical(id),
                        params,
                    } => {
                        assert_eq!(id.dotted_str(), "host.reconcile");
                        // `target` flows through; `kind`/`action`/`role` are consumed.
                        assert_eq!(
                            params.get("target").and_then(|v| v.as_str()),
                            Some("lab8gb")
                        );
                        assert!(!params.contains_key("role"));
                    }
                    _ => panic!("inner should be Execute"),
                }
            }
            _ => panic!("expected Confirm"),
        }
    }

    #[test]
    fn lowers_flow_schedule_to_schedule_wrapping_execute() {
        let ir = line("flow.schedule kind=drift_review target=core after=1h")
            .to_ir_primitive()
            .unwrap();
        match ir {
            IRPrimitive::Schedule { action, trigger } => {
                assert_eq!(trigger.0, "1h");
                match *action {
                    IRPrimitive::Execute {
                        action: ActionKind::Canonical(id),
                        ..
                    } => assert_eq!(id.dotted_str(), "flow.drift_review"),
                    _ => panic!("inner should be Execute"),
                }
            }
            _ => panic!("expected Schedule"),
        }
    }

    #[test]
    fn lowers_cancel_for_both_cmd_and_flow_namespaces() {
        let a = line("cmd.cancel id=abc").to_ir_primitive().unwrap();
        let b = line("flow.cancel id=xyz").to_ir_primitive().unwrap();
        assert!(matches!(a, IRPrimitive::Cancel { ref id } if id == "abc"));
        assert!(matches!(b, IRPrimitive::Cancel { ref id } if id == "xyz"));
    }

    #[test]
    fn unmapped_verb_is_a_closed_error() {
        let err = line("space.ritual key=v").to_ir_primitive().unwrap_err();
        match err {
            IrLoweringError::UnmappedVerb { kind } => assert_eq!(kind, "space.ritual"),
            _ => panic!("expected UnmappedVerb"),
        }
    }

    #[test]
    fn missing_required_arg_is_a_closed_error() {
        let err = line("lab.collect kind=events target=core")
            .to_ir_primitive()
            .unwrap_err();
        match err {
            IrLoweringError::MissingArg { kind, arg } => {
                assert_eq!(kind, "lab.collect");
                assert_eq!(arg, "window");
            }
            _ => panic!("expected MissingArg"),
        }
    }

    #[test]
    fn roundtrip_line_through_display_reparses_equal() {
        let original = line(
            r#"lab.prioritize kind=attention target=core window=1h infer=lab512 emit=thread:t123"#,
        );
        let rendered = original.to_string();
        let again = parse_line(&rendered).unwrap();
        assert_eq!(original, again);
    }

    #[test]
    fn ast_json_roundtrip_is_stable() {
        let p = parse_program(
            "flow.verify_report target=lab8gb infer=lab512\n  confirm.request action=host.reconcile role=admin\n",
        )
        .unwrap();
        let s = serde_json::to_string(&p).unwrap();
        let back: OperationalProgram = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }
}
