# Constitutional runtime

This is the name for the execution model where **no material action is valid merely because it is executable**. The runtime is **not sovereign**; it is subordinate to an operational constitution.

## Binding order

1. **Canonical semantics** — intermediate representation (IR), not ad hoc strings
2. **Capability jurisdiction** — manifests state what each substrate can realize
3. **Policy class** — regimes of admissibility (A/B/C/D); **D** is *reserved*: representable in graphs but **not** admitted at the normal runtime execution boundary (use deliberation or authority paths).
4. **Authority rules** — who may confirm or escalate
5. **Evidence obligations** — what must be recorded for an act to *close*
6. **Explicit boundaries** between cognition, deliberation, and execution

## Formulations

- *The runtime is constitutional because execution is not sovereign. It is bound by a prior semantic, policy, authority, capability, and evidence order.*
- *A constitutional runtime is one in which no material action is valid merely because it is executable; it must also be semantically admissible, policy-permitted, capability-realizable, and evidentially accountable.*

## Separation of powers (conceptual)


| Layer              | Power                                                |
| ------------------ | ---------------------------------------------------- |
| Place agent        | Cognitive — proposes structure and intent            |
| Decision / planner | Deliberative — chooses paths and substrate           |
| Lowerer / executor | Executive — does not “legislate” beyond the contract |
| Evidence system    | Archival — audit, replay, justification              |


## Pipeline

```
Strong grammar
  → semantic constitution (IR)
  → admissibility / validation
  → jurisdiction / capability matching
  → deliberative lowering (when needed)
  → executive lowering
  → execution
  → constitutional record (evidence)
```

## Practical consequences

- **`Decide`** does not collapse into the executive lowerer; it needs a planner or flow compiler.  
- **`Persist`** is not an invisible default; durability is a constitutional concern.  
- **Capability mismatch** is lack of material jurisdiction, not a random runtime error.  
- **Policy block** is operational inadmissibility, not a superficial guard.  
- **Missing evidence** means the act has not constitutionally closed.

## Trust

*Trust runs natively above software*: IR, policy, capability, authority, and evidence are **structural** and ordered **before** execution. Legitimacy is conjunction of semantic validity, policy admissibility, capability realizability, authority sufficiency, and evidence contract—not a patch layered after the fact.

## Ingress

Natural language ingress is a **progressive refinement** under cost constraints, not a single translation step. Premium LLM use is a **constitutional exception path** (planned, rare, logged)—not the default operating loop.

**Related:** [Documentation index](../README.md) · [IR and lowering](ir-and-lowering.md) · [Crate reference](crate-reference.md)
