# Minilab Max Automation Map

This repository now starts from the constitutional runtime reference and adds the
first executable Minilab body: a Rust-native `minilab` CLI.

## Canonical hierarchy

1. **LogLine** is the constitutional grammar.
2. **Rust CLI** is the first executable institutional body.
3. **Constitutional Runtime** governs admissibility, authority, evidence, and closure.
4. **Tower / minilab.work** governs operational power and machine consequence.
5. **LABs** execute local work and never govern.
6. **Cloudflare Gateway** is public edge transport, not a sovereign brain.
7. **Receipts** are memory and proof: receipts beat stories.

## Implemented v0 slice

The `minilab-cli` crate implements local, non-destructive probes for the plan's
Sprint A through Sprint J command surface:

- `minilab logline compile|walk|digest`
- `minilab receipt emit|validate|index`
- `minilab workorder render-prompt|probe|to-logline`
- probe surfaces for identity, alias, gate, admission, gateway, lab,
  expedition, incident, maintenance, ghost, audit, snapshot, restore, release,
  package, build, milestone, adr, and registry

These commands deliberately emit candidates or local receipts only. They do not
claim external execution, production admission, or provider success.

## Nine-slot act contract

Every material CLI input must preserve the mandatory nine slots:

```text
who
did
this
when
confirmed_by
if_ok
if_doubt
if_not
status
```

Text `.logline` files may use `key = value` lines. JSON files may serialize the
same shape directly. The CLI rejects acts missing any mandatory slot.

## Evidence boundary

A receipt is encoded as:

```text
LogLine act + result + evidence + transport + hashes
```

The first hash is `receipt_without_hashes_sha256`, computed after clearing the
hash map so validation can replay the digest deterministically.
