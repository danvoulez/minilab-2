# Migrations

These migrations stay **flat and ordered** on purpose.

Why:

- most migration tooling expects a single ordered directory
- execution order matters more here than conceptual grouping
- schema history should stay obvious during incident review and replay

## Sequence

| Migration | Purpose |
| --- | --- |
| `001_minilab_canonical.sql` | Initial canonical GTM entities and evidence ledger |
| `002_minilab_extended_entities.sql` | Extended operational entities for the Minilab wedge |
| `003_account_dedupe_key_unique.sql` | Enforce stable account dedupe |
| `004_campaign_claims_profile_unique_names.sql` | Protect campaign and claims-profile naming uniqueness |
| `005_evidence_idempotency.sql` | Add idempotency semantics to evidence writes |
| `006_message_family_unique_label.sql` | Enforce per-campaign message-family uniqueness |
| `007_upsert_account_rpc.sql` | Canonical account upsert RPC |
| `008_contact_phone_e164.sql` | Add WhatsApp-capable contact phone field |
| `009_reply_ingest_metadata.sql` | Add reply ingest metadata and correlation support |
| `010_host_pairing_slice.sql` | Host pairing slice: challenge, token hash, and pairing ledger |
| `011_install_reconcile_slice.sql` | Install reconciliation slice: manifest diff, step tracking |
| `012_runtime_sim_branching.sql` | Runtime simulation branching: sim_mode and branch_id columns |
| `013_agent_runtime_postgres.sql` | Agent runtime Postgres tables and typed online LogLine rows |

## Rule

Do not reorganize these into subfolders unless the migration runner itself changes.  
Conceptual grouping belongs in `docs/`; execution history belongs here.
