# ChatGPT Place Adapter Layer

**Status:** proposed · **Type:** operational integration memo · **Scope:** constitutional-runtime + external premium substrate

**Target baseline for implementation assumptions:** ChatGPT Business as of **2026-04-23**.

**Product availability note:** workspace agents were publicly announced by OpenAI on **2026-04-22** and are in research preview across ChatGPT Business, Enterprise, Edu, and Teachers plans.

This memo defines the operational shape of the `X` in the equation:

> `Place ChatGPT + runtime da OpenAI + X = Place Agent aproximado`

where `X = Constitutional Place Adapter Layer`.

This is not a second runtime core. The constitutional runtime remains sovereign. The adapter exists to make a ChatGPT-backed premium surface behave like a governed Place inside the Minilab system.

Under the current public OpenAI product assumptions assumed by this memo:

- the OpenAI side already supplies the premium agent UI, premium model substrate, cloud execution, browser, terminal, code execution, and shared workspace-agent surface;
- `workspace agents` are available in research preview for ChatGPT Business and can be shared, scheduled, and connected to Slack;
- remote MCP / app integration is the supported extension boundary;
- local MCP is not assumed;
- `agent mode` is not assumed as the write-path integration surface for custom tools;
- OpenAI documentation and UI currently use both `apps` and `connectors` across different surfaces.

The adapter therefore treats ChatGPT as a premium substrate and a hosted operator surface, not as a constitutional authority.

OpenAI-side confirmation modals for write / modify actions are treated as an additional product safeguard, not as a substitute for Minilab-side policy, confirmation, or admissibility.

---

## 1. Equation

### Full place

`Place normal + nosso agent runtime próprio = Y`

### Premium-assisted place

`Place ChatGPT + runtime da OpenAI + X = Y aproximado`

The approximation is useful and strategic, but not identical to the full native Place Agent path.

---

## 2. Decision

The ChatGPT-backed Place is admitted as a **premium-assisted Place**, not as a new runtime core.

The adapter layer must provide:

1. canonical place identity;
2. a policy-governed bridge into the existing grammar / IR pipeline;
3. official session / run / checkpoint objects owned by Minilab;
4. normalized outputs that never mutate canonical truth directly.

The OpenAI substrate may produce useful work, but that work enters Minilab only through closed output classes and governed submission paths.

For this memo, the preferred OpenAI surface is:

`ChatGPT Business workspace agents + remote MCP-backed apps`

not:

`agent mode + custom write-path apps`

because current OpenAI documentation does not guarantee the latter path.

For this architecture, `workspace agents` are the primary premium surface, and developer-mode remote MCP apps are the primary extension boundary. `Agent mode` is explicitly out of scope as a custom write-path integration surface under current OpenAI behavior.

---

## 3. Components

| Service | Suggested host | Required | Function |
|---|---|---|---|
| `minilab-place-adapter` | `LAB 8GB` | yes | Orchestrates the ChatGPT Place inside Minilab |
| `minilab-mcp-query` | `LAB 8GB` | yes | Exposes read access from Minilab into ChatGPT apps |
| `minilab-mcp-command` | `LAB 8GB` | yes | Exposes governed action submission into Minilab apps |
| `minilab-mcp-artifacts` | `LAB 8GB` | yes | Receives summaries, artifacts, and external outputs |
| `place-policy-pack` | repo / config | yes | Defines identity, limits, and policy of the Place |
| `agent-session-store` | Postgres via Minilab backend / store | yes | Persists official session / run / checkpoint objects as typed rows |
| `chatgpt_workspace` place entry | canonical place catalog | yes | Registers the new canonical Place |
| richer terminal sync | later | no | Improves parity with the native Place Agent |
| full `infer/exec/emit` bridge | later | no | Closes stronger equivalence with the native runtime |

---

## 4. Responsibilities

### 4.1 `minilab-place-adapter`

This is the heart of `X`.

Responsibilities:

- resolve `place_slug -> CanonicalPlaceId`;
- load `PlaceProfile`;
- open / continue `AgentSession`;
- open `AgentRun`;
- register `Checkpoint`;
- decide whether a result is advisory, confirm-required, handoff, or governed act;
- normalize output coming back from ChatGPT;
- translate tool / app events into Minilab structured intent and then into grammar / IR on the Minilab side.

Inputs:

- user message;
- `place_id`;
- optional context / `session_id`;
- ChatGPT-produced result;
- relevant tool / app events.

Outputs:

- `AgentTurnOutcome`;
- `AgentSessionSnapshot`;
- `AgentCheckpoint[]`;
- `ArtifactRef[]`;
- `HandoffResult`, when applicable.

### 4.2 `minilab-mcp-query`

Purpose: give ChatGPT visibility into the Minilab world through a remote MCP-backed app.

Minimum tools:

- `get_place_status(place_id)`
- `get_place_context(place_id)`
- `get_agent_session(session_id)`
- `get_recent_timeline(session_id | place_id)`
- `get_terminal_session(id)`
- `search_runtime_docs(query)`
- `fetch_runtime_doc(id)`

Protocol / exposure assumptions:

- hosted remotely, not locally;
- imported into ChatGPT as an app backed by a remote MCP server;
- protocol: `SSE` or `streaming HTTP`.

### 4.3 `minilab-mcp-command`

Purpose: force ChatGPT to ask Minilab to act rather than acting outside the constitutional boundary.

Minimum tools:

- `submit_place_intent(place_id, structured_intent)`
- `start_governed_handoff(place_id, task_kind, payload)`
- `continue_terminal_session(id, command)`
- `request_confirmation(action_ref)`
- `submit_operational_action(action_kind, params, policy_context)`

Rule:

This service is not a bypass. It converts externally-originated intent into governed submission on the Minilab side.

Read-path access may be served by remote MCP-backed apps used from ChatGPT. Write-path intent must still enter Minilab through governed submission surfaces owned by Minilab, even when initiated from ChatGPT.

Authentication assumptions for remote MCP-backed apps:

- `OAuth`
- `No Authentication`
- `Mixed Authentication`

Recommended default for Minilab command surfaces: `OAuth`.

### 4.4 `minilab-mcp-artifacts`

Purpose: bring the work performed in ChatGPT back into Minilab's official world.

Minimum tools:

- `create_artifact(...)`
- `store_external_result(...)`
- `attach_chatgpt_summary(...)`
- `append_evidence_note(...)`
- `attach_run_output(...)`

Accepted output classes:

- `Advisory`
- `Proposal`
- `Artifact`
- `ExecutionSummary`
- `HandoffResult`

### 4.5 `place-policy-pack`

Purpose: define who the Place is and what it may do.

Minimum contents:

- `CanonicalPlaceId`
- `PlaceSlug`
- `DisplayLabel`
- capability envelope
- allowed task classes
- denied task classes
- confirmation rules
- allowed output classes
- default `infer_surface`
- default `emit_surface`

Example:

```yaml
place_id: chatgpt_workspace
slug: chatgpt-workspace
label: ChatGPT Workspace
mode: premium_assisted_place
default_infer_surface: chatgpt_workspace
allowed_outputs:
  - advisory
  - proposal
  - artifact
  - handoff_result
requires_confirmation_for:
  - material_actions
  - terminal_continue
forbidden:
  - canonical_truth_write
```

---

## 5. Grammar bridge

The adapter must not rely on the ChatGPT side writing free-form operational grammar as authoritative text.

Preferred path:

`tool calls -> structured intent -> Minilab grammar / IR`

In OpenAI product terms, these are expected to be app / MCP tool calls, not free-form textual commands.

Example mental model:

```json
{
  "tool": "submit_place_intent",
  "args": {
    "place_id": "lab512",
    "intent_kind": "drift_review",
    "target": "runtime",
    "window": "24h",
    "infer_surface": "chatgpt_workspace"
  }
}
```

The adapter then converts this into the Minilab-side equivalent:

- operational grammar, such as `flow.drift_review ... infer=chatgpt_workspace`; or
- a direct IR / structured command submission path.

The adapter owns this compilation boundary. The ChatGPT side does not become the grammar authority.

Important product boundary:

- `workspace agents` can be configured with apps, custom MCPs, skills, files, and channels;
- `agent mode` should not be treated as the custom write-path surface for this architecture;
- `deep research` may use custom apps for read / fetch actions, but not for write actions.

---

## 6. Output normalization

The ChatGPT side may not write canonical truth directly.

Every result is normalized into one of a few closed classes:

- `Advisory`
- `Proposal`
- `Artifact`
- `ExecutionSummary`
- `HandoffResult`

This separation prevents confusion between:

- useful work produced by the premium substrate; and
- official state mutation inside Minilab.

---

## 7. Official runtime objects

Even if OpenAI has its own notion of runs or tasks, Minilab owns the official shape.

Minimum official objects:

- `AgentSession`
- `AgentRun`
- `AgentCheckpoint`

The UI-facing contract should continue to expose the familiar objects already expected by the frontend:

- `AgentRuntimeSessionSnapshot`
- `AgentRuntimeTurnResult`
- `AgentRuntimeCheckpoint`
- `AgentRuntimeTerminalSession`
- `AgentRuntimeAction`
- `AgentRuntimeAuditTrail`
- `AgentRuntimeEffectivePolicy`

The Minilab-side session is the source of truth for product behavior. Official adapter state is persisted online in Postgres as typed session, run, checkpoint, audit-event, and nine-slot `runtime_loglines` rows. The adapter must not treat a generic JSONB document as the official storage contract.

---

## 8. Minimum execution flows

### 8.1 Advisory flow

1. user talks to the ChatGPT-backed Place;
2. ChatGPT consults `mcp-query`;
3. ChatGPT returns useful output;
4. `minilab-place-adapter` records online in Postgres:
   `AgentSession`, `AgentRun`, `Checkpoint`, `Advisory`, and the corresponding typed nine-slot LogLine row;
5. the UI renders the turn as an official runtime turn.

### 8.2 Governed handoff flow

1. user requests something material;
2. ChatGPT recognizes the intent;
3. it calls `mcp-command.submit_place_intent(...)`;
4. the adapter converts that into Minilab-side command / grammar / IR;
5. the constitutional runtime decides:
   `confirm`, `accept`, `deny`, or `handoff`;
6. the result returns as `HandoffResult` or `ExecutionSummary`.

### 8.3 Artifact flow

1. ChatGPT produces a report, file, or synthesis;
2. it calls `mcp-artifacts.create_artifact(...)`;
3. the adapter links the artifact to the run;
4. the UI surfaces it as part of the official session history backed by Postgres runtime rows.

---

## 9. What OpenAI supplies vs what Minilab supplies

| Capability | OpenAI supplies? | Minilab supplies? |
|---|---|---|
| premium agent UI | yes | no |
| premium LLM | yes | no |
| browser / terminal / code execution | yes | no |
| shared workspace agents | yes | no |
| remote MCP app support in the host product | yes | no |
| Minilab remote MCP servers / apps | no | yes |
| canonical place identity | no | yes |
| place policy | no | yes |
| grammar / IR bridge | no | yes |
| official session / run / checkpoint | no | yes |
| constitutional truth / authority | no | yes |
| output normalization | no | yes |
| evidence / artifact ingest | no | yes |

The OpenAI side already provides the premium surface. The constitutional adapter is what makes that surface incorporable into Minilab.

---

## 10. Prerequisites

### 10.1 On the ChatGPT side

- ChatGPT Business workspace;
- workspace agents enabled for the workspace;
- developer mode enabled when needed;
- remote MCP-backed apps configured;
- workspace permissions / apps enabled as required.

Important terminology and product note:

- OpenAI currently uses both `apps` and `connectors` across product and documentation surfaces;
- this memo uses `apps` as the preferred shorthand for remote MCP-backed integrations, while acknowledging that some official UI and help surfaces still say `connectors`.

### 10.2 On the Minilab side

- an always-on public or semi-public host for remote MCP;
- secure authentication;
- basic logging and health checks;
- storage for `AgentSession` / `AgentRun` / `AgentCheckpoint`;
- a canonical place-id resolver;
- a versioned policy pack.

---

## 11. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| confusing ChatGPT output with truth store | high | normalize all outputs; never allow direct canonical truth writes |
| excessive coupling to the external UX | medium | keep `minilab-place-adapter` as the canonical boundary |
| remote MCP unavailability | high | run on stable always-on host with health checks |
| agent mode not covering the desired write path | high | use workspace agents + remote apps for the primary path; keep command plane on Minilab side |
| divergence between OpenAI-side session and Minilab-side session | medium | Minilab `AgentSession` remains authoritative |
| policy too loose | high | enforce `place-policy-pack` and governed `mcp-command` path |
| app action inventory drifting silently | high | treat app action inventory as versioned operational surface; require review / refresh before enabling MCP action changes |

---

## 12. V1 and V2

### 12.1 V1

Minimum set required to operate:

- `chatgpt_workspace` as canonical Place;
- ChatGPT Business workspace agent as the premium surface;
- `place-policy-pack`;
- `minilab-place-adapter`;
- `minilab-mcp-query`;
- `minilab-mcp-command`;
- `minilab-mcp-artifacts`;
- `AgentSession`;
- `AgentRun`;
- `AgentCheckpoint`;
- closed output classes.

Outcome:

The system gains a usable premium-assisted Place.

### 12.2 V2

Additional improvements:

- richer terminal sync;
- richer `infer/exec/emit` bridge;
- finer policy adapter;
- richer evidence capture;
- better projections;
- stronger parity with the native Place Agent.

---

## 13. Deployment recommendation

Recommended coordination host: `LAB 8GB`.

Why:

- it is a natural always-on coordination node;
- it avoids pushing this integration seam into `LAB 512`;
- it is better suited for lightweight coordination than for local-heavy inference work.

Initial stack to bring up:

1. `minilab-place-adapter`
2. `minilab-mcp-query`
3. `minilab-mcp-command`
4. `minilab-mcp-artifacts`

Recommended build order:

1. define `chatgpt_workspace` in the canonical place catalog;
2. create the ChatGPT Business workspace agent surface;
3. write `place-policy-pack`;
4. bring up `mcp-query`;
5. bring up `mcp-command`;
6. implement the adapter;
7. enable ChatGPT developer mode and configure remote MCP-backed apps;
8. bring up `mcp-artifacts`;
9. test the advisory flow;
10. test the governed handoff flow;
11. test artifact ingest.

---

## 14. Final formula

### Full native path

`Place + nosso backend oficial + nosso runtime = Place Agent pleno`

### Premium-assisted path

`Place ChatGPT + OpenAI ChatGPT Business workspace agent + Constitutional Place Adapter Layer = Place Agent assistido-premium`

Name of `X`:

> `Constitutional Place Adapter Layer`

---

## 15. Verified OpenAI product assumptions

This memo assumes the following product assumptions verified against OpenAI's official docs on **2026-04-23**:

1. `workspace agents` were announced on **2026-04-22** and are available in research preview, including ChatGPT Business.
2. workspace agents can use apps, custom MCPs, skills, files, and channels.
3. remote MCP-backed apps support `SSE` and `streaming HTTP`.
4. remote MCP-backed apps support `OAuth`, `No Authentication`, and `Mixed Authentication`.
5. local MCP is not supported.
6. `search` and `fetch` are not generally required for full MCP in developer mode.
7. `search` and `fetch` remain the compatibility shape for deep research / company knowledge style read-only integrations.
8. `agent mode` should not be assumed to use custom apps for write actions.

If OpenAI changes those assumptions, this memo should be amended before implementation assumptions drift.

---

## 16. OpenAI-side operational caveats

- Workspace agents are in research preview, so product behavior, UI, and permissions may continue to change.
- Local MCP is unsupported; only remote servers are supported.
- `Agent mode` should not be assumed to use custom apps for write actions.
- Published app action sets should be treated as reviewable operational surfaces rather than ambient capability: new actions are disabled by default, updates to existing actions are shown as diffs, and MCP-backed action changes require explicit review / refresh before they are enabled.

---

## 10. Landed HTTP surface — 2026-05 sprint

The adapter now has a first in-repo proving surface in `minilab-api`:

| Surface | Method/path | Purpose |
|---|---|---|
| Agent profile | `GET /api/agent-runtime/places/{place_id}` | Resolves the embedded `PlaceProfile` for the ChatGPT-backed Place. |
| Agent message | `POST /api/agent-runtime/places/{place_id}/messages` | Opens or resumes an official Minilab agent session, classifies the output, and runs the candidate through Strong Grammar → IR → plan → scripted dispatch evidence before returning the acknowledgement. |
| Session list | `GET /api/agent-runtime/sessions` | Lists in-memory session snapshots for operator/debugging visibility. |
| Session read | `GET /api/agent-runtime/sessions/{session_id}` | Reads the official session/run/checkpoint snapshot, including runtime pipeline evidence. |
| MCP query | `POST /mcp/query` | Remote MCP read surface for profile/session/timeline/doc visibility. |
| MCP command | `POST /mcp/command` | Remote MCP governed-command surface; stages proposals and confirmations, never direct execution. |
| MCP artifacts | `POST /mcp/artifacts` | Remote MCP artifact/result ingress; normalizes output into closed output classes. |

This is still intentionally in-memory and adapter-shaped. It proves the boundary and route contract without claiming durable agent-session storage. The next sprint should move `AgentRuntimeStore` behind the same evidence/persistence discipline used by the constitutional slices.
