# Doppler Project Plan — `minilab_huge_mini`

This document defines the Doppler structure for the current mono-repo, which still contains both:

- **Minilab Huge Mini** as the company/runtime implementation
- **constitutional-runtime** as the first and official implementation of the constitutional core

The design is based on the existing repository documentation, especially:

- [Minilab architecture](../minilab/architecture.md)
- [Repository layout and split criteria](../repo/layout-and-split-criteria.md)
- [Slice roadmap](slice-roadmap.md)
- [Canonical entities](../minilab/canonical-entities.md)
- [Manifesto](../minilab/manifesto.md)
- [Repository README](../../README.md)

## Why one Doppler project is correct now

The repo is intentionally still **one implementation center**. The docs say the same repo remains correct while:

- the constitutional runtime
- the Minilab application layer
- the GTM wedge
- the canonical entities and migrations
- the operational docs

are still tightly coupled in one body of work.

So the Doppler shape should follow that same rule:

- **one Doppler project now**
- **clear internal config boundaries inside that project**
- **split later only if the repo split criteria become true**

## Project created

Canonical Doppler project identifier:

- `minilab_huge_mini`

Human meaning:

- "Minilab Huge Mini" as business/runtime project
- plus the **constitutional-runtime official implementation**

## Environment

The project currently uses one real lifecycle environment:

- `prd`

This is the right shape for the repo as it exists today because there is only one live operational environment, and the user explicitly rejected lifecycle theater.

## Config topology

The environment keeps one **root config** and four explicit **surface configs**.

### Root config

- `prd`

Purpose:

- shared, environment-wide defaults
- non-secret operational config
- repo identity and safety defaults

These root configs should hold values that are valid across more than one component in the same environment.

### Surface configs

Inside `prd`:

- `${env}_constitutional_runtime`
- `${env}_minilab_store`
- `${env}_minilab_api`
- `${env}_gtm_ops`

These exist because the mono-repo contains **distinct configuration jurisdictions**, even before a repo split.

## What each config is for

### Root environment config

Scope:

- shared runtime defaults
- environment stage identity
- safe behavioral defaults
- non-secret infra knobs shared across consumers

Examples already seeded:

- `MINILAB_DOPPLER_SCHEMA_VERSION`
- `MINILAB_REPO_TOPOLOGY`
- `MINILAB_IMPLEMENTATION_CANONICAL`
- `MINILAB_BUSINESS_RUNTIME`
- `MINILAB_DEPLOY_STAGE`
- `MINILAB_SIM_MODE`
- `MINILAB_OUTBOUND_PROVIDER`
- `MINILAB_HTTP_BIND_ADDR`
- `MINILAB_HTTP_TIMEOUT_SECS`
- `MINILAB_HTTP_MAX_TWILIO_BODY_BYTES`
- `MINILAB_HTTP_MAX_SENDGRID_BODY_BYTES`
- `MINILAB_REPLY_EMAIL_LOCALPART`

Rule:

- if multiple binaries in the same environment need it, prefer the root config
- if only one surface owns it, move it to that surface config

### `${env}_constitutional_runtime`

Scope:

- constitutional semantics
- runtime governance inputs
- structured policy / registry / manifest references
- configuration that belongs to the **constitutional boundary**, not the transport/infrastructure edge

Why this exists:

The docs are clear that the constitutional layer owns:

- IR meaning
- policy
- capability
- evidence duties
- runtime admissibility

This config should become the home for structured values such as:

- `MINILAB_COMPANY_REGISTRY_JSON`
- `MINILAB_CHANNEL_POLICIES_JSON`
- `MINILAB_ESCALATION_RULES_JSON`
- `MINILAB_CAPABILITY_MANIFEST_JSON`
- `MINILAB_POLICY_BUNDLE_VERSION`
- `MINILAB_EVIDENCE_BACKEND`

Important:

- this config is **not primarily for secrets**
- it is primarily for **governed runtime config**

### `${env}_minilab_store`

Scope:

- persistence and ledger writes
- ingest/score/send/reply workers
- outbound transport credentials
- store-owned operational config

Why this exists:

`minilab-store` is the stateful operational layer for:

- Supabase persistence
- evidence ledger writes
- scoring
- policy gate
- outbound dispatch
- reply ingest

Expected keys:

- `SUPABASE_URL`
- `SUPABASE_SECRET_KEY`
- legacy fallback: `SUPABASE_SERVICE_KEY`
- `SENDGRID_API_KEY`
- `TWILIO_SENDGRID_FROM_EMAIL`
- `TWILIO_SENDGRID_FROM_NAME`
- `TWILIO_SENDGRID_BASE_URL`
- `TWILIO_ACCOUNT_SID`
- `TWILIO_AUTH_TOKEN`
- `TWILIO_WHATSAPP_FROM`
- `MINILAB_REPLY_EMAIL_DOMAIN`
- `MINILAB_REPLY_EMAIL_LOCALPART`
- `SENDGRID_PARSE_PUBLIC_KEY`

### `${env}_minilab_api`

Scope:

- permanent HTTP edge
- health/readiness
- verified inbound webhooks
- HTTP-facing knobs and webhook credentials

Why this exists:

The repo now has a permanent `axum` edge in `minilab-api`, and the user explicitly rejected temporary inbound implementations.

Expected keys:

- `SUPABASE_URL`
- `SUPABASE_SECRET_KEY`
- `MINILAB_PUBLIC_BASE_URL`
- `MINILAB_HTTP_BIND_ADDR`
- `MINILAB_HTTP_TIMEOUT_SECS`
- `MINILAB_HTTP_MAX_TWILIO_BODY_BYTES`
- `MINILAB_HTTP_MAX_SENDGRID_BODY_BYTES`
- `TWILIO_AUTH_TOKEN`
- `SENDGRID_PARSE_PUBLIC_KEY`

### `${env}_gtm_ops`

Scope:

- business wedge configuration
- explicit GTM judgments
- ICP/persona/channel/offer/policy choices that the docs say must not remain hidden in code

Why this exists:

The slice roadmap explicitly says business judgments affecting admissibility must be captured in:

- policy
- config
- or ticket notes

This config is where those business/runtime decisions should live while the repo is still unified.

Recommended structured keys:

- `MINILAB_GTM_WEDGE_JSON`
- `MINILAB_PRIMARY_ICP`
- `MINILAB_PRIMARY_PERSONA`
- `MINILAB_PRIMARY_CHANNEL`
- `MINILAB_PRIMARY_REGION`
- `MINILAB_PRIMARY_OFFER`
- `MINILAB_PRIMARY_BOOKING_PATH`
- `MINILAB_DEFAULT_CLAIMS_PROFILE`
- `MINILAB_GTM_THROTTLE_PROFILE_JSON`

Important:

- do not invent these values silently in code
- do not store long prose documents here
- prefer compact JSON or stable identifiers that point to committed docs or schemas

## Secret vs config classification

### Secrets

These should remain secret / masked / restricted:

- `SUPABASE_SECRET_KEY`
- legacy `SUPABASE_SERVICE_KEY`
- `SENDGRID_API_KEY`
- `TWILIO_AUTH_TOKEN`

### Sensitive but often non-secret operational values

These are usually config, but still operationally important:

- `SUPABASE_URL`
- `TWILIO_ACCOUNT_SID`
- `TWILIO_WHATSAPP_FROM`
- `TWILIO_SENDGRID_FROM_EMAIL`
- `MINILAB_PUBLIC_BASE_URL`
- `SENDGRID_PARSE_PUBLIC_KEY`

### Plain config

- `MINILAB_SIM_MODE`
- `MINILAB_OUTBOUND_PROVIDER`
- `MINILAB_HTTP_*`
- runtime registry/policy JSON
- GTM wedge metadata

## Current seeded baseline

The Doppler project now has:

- project `minilab_huge_mini`
- environment `prd`
- surface configs for runtime, store, api, and gtm ops inside `prd`
- root environment defaults seeded for real operation

Current safety defaults:

- `prd`: `MINILAB_SIM_MODE=production`, `MINILAB_OUTBOUND_PROVIDER=auto`

This is intentional. Production remains governed by explicit credentials, policy posture, and runtime admissibility, not by fake environments.

## Recommended next fill order

1. Fill `prd_minilab_store`
   Add `SUPABASE_SECRET_KEY`, outbound provider credentials, and reply-domain config.

2. Fill `prd_minilab_api`
   Add `SUPABASE_SECRET_KEY`, `MINILAB_PUBLIC_BASE_URL`, webhook verification inputs.

3. Fill `prd_constitutional_runtime`
   Add structured JSON or IDs for company registry, channel policies, escalation rules, and evidence backend choice.

4. Fill `prd_gtm_ops`
   Add one narrow wedge only: one ICP, one persona, one offer, one channel, one region, one booking path.

5. Go live deliberately in `prd`
   Keep `MINILAB_OUTBOUND_PROVIDER=auto`, but only promote the real provider credentials and policy posture when the wedge is admitted and audited.

## Governance rules for this Doppler project

1. Doppler is not only a secret bag.
   It is the operational config boundary for this mono-repo.

2. The root environment config is shared baseline only.
   Component-specific ownership belongs in the surface configs.

3. Business judgment must not hide in code.
   If a choice affects policy, admissibility, or claims, it belongs in `gtm_ops`, runtime config, or committed docs.

4. Runtime semantics must not be mixed with transport credentials.
   `constitutional_runtime` and `minilab_store` are different jurisdictions even in one repo.

5. Safe by default.
   Real outbound and real production mutation should not appear just because credentials exist.

6. Same repo now, split later.
   If the repo split criteria become true, the Doppler project should split along the same constitutional boundaries.
