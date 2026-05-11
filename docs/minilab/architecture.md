# Minilab architecture (working definition)

Minilab is a **personal distributed system** with a human interface, two isolated network hosts (`lab8gb`, `lab512`), one admin machine (`lab256`), one generic agent per network host, **Supabase** for durable coordination, a **signed manifest** as the runtime contract, a durable **work model** for chat and commands, and **routed inference** across hosts.

## Design rule

**Adopt stable external foundations for commodity problems; reserve local definition for product semantics.**

## What Minilab is not

- A from-scratch reinvention of trust, discovery, enrollment, and rollout  
- “Chat app with infra behind it”  
- A sovereign HTTP control plane  
- A UI that owns system authority

## Adopted foundations (freeze as policy)

Use mature patterns and libraries for commodity mechanics; Minilab owns **adapters** and **payload semantics**:


| Concern                        | Direction                                                 |
| ------------------------------ | --------------------------------------------------------- |
| Manifest trust and publication | TUF-style (e.g. `tuf-js`)                                 |
| LAN discovery                  | mDNS (e.g. `mdns-sd`), service like `_minilab-agent._tcp` |
| Enrollment / credentials       | Headscale-inspired approval and lifecycle                 |
| Pairing / bootstrap trust      | Nabto-inspired local ceremony                             |
| Rollout / verify / evidence    | Mender-inspired lifecycle                                 |


## Local sovereignty (do not outsource)

- Minilab manifest **schema** and meaning  
- **Operational grammar** and IR  
- Supabase **work model** (commands, leases, idempotency, append-only events)  
- Host topology and roles (`lab8gb`, `lab512` as network hosts; `lab256` as admin machine)  
- **Places** as the single presentation lens  
- Authority model: human, LLM, agent, database, runtime contract

## Acceptance themes (project-level)

- **Runtime:** manifest published under a fixed trust model; runtime reads a single validated snapshot  
- **Discovery:** hosts advertise the agent; peers can discover and interpret  
- **Enrollment / pairing:** explicit rituals; credentials issuable and revocable; pairing separated from normal operation  
- **Work model:** persisted commands, leases, retries, idempotency, append-only events  
- **Rollout:** releases, installations, verify, readable evidence  
- **Interface:** one UI; Places as lens; UI not an authority surface

## Hard operational constraints

- **External Supabase is the source of truth** for infra coordination and recovery.
- **Installation must be wizard-driven**, not dependent on fragile manual bring-up.
- **Updates must come from signed artifacts** published in external storage and verified before install.
- **UI is mandatory** for host health, readiness, last action, install state, and evidence visibility.

These constraints exist because repeated manual infra installation attempts failed. Reliability now depends on making install/update/observe first-class product surfaces.

## Relation to this crate

**Constitutional Runtime** (this library) is the **semantic and mechanical boundary** inside that world: IR, policy, capability, lowering, and evidence—not the full Minilab stack (networking, Supabase schema, UI).

**Related:** [Documentation index](../README.md) · [Constitutional runtime](../runtime/constitutional-runtime.md) · [IR and lowering](../runtime/ir-and-lowering.md)
