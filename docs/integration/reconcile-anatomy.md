# Reconcile Anatomy — `install.reconcile`

**Status:** accepted · **Type:** pattern memo · **Scope:** `install.reconcile` slice · **Date:** 2026-05-11

This memo captures the first Reconcile-shaped constitutional slice. It extends the slice-pattern catalog without pretending that desired/applied convergence is the same shape as a single admissibility-governed act.

---

## 1. Decision

`install.reconcile` is the pioneer Reconcile slice. It accepts a desired installation manifest, compares it with an optional applied manifest snapshot, applies only divergent payload-service steps, and closes the evidence chain under one `correlation_id`.

The landed evidence vocabulary is:

1. `install.reconcile.planned`
2. `install.reconcile.step.applied`
3. `install.reconcile.reconciled`
4. `install.reconcile.failed`

The ledger remains the constitutional source of truth. The optional `installation_state` table is an operational cache only.

---

## 2. Reconcile stations

| Station | `install.reconcile` concrete form |
|---|---|
| Minimal input | `installation_id`, `host_id`, `desired_manifest`, optional `applied_manifest`, `correlation_id` |
| Canonical lowering | `ActionKind::Canonical("install.reconcile")` lowers to `OperationalCommand { namespace: "install", verb: "reconcile", target_runtime: Platform }` |
| Dispatch handoff | `dispatch_operational_command` decodes the typed command args and calls `submit_install_reconcile` |
| Planning | Manifest validation and desired/applied diff emit `install.reconcile.planned` |
| Step convergence | One `install.reconcile.step.applied` row per divergent payload service |
| Terminal closure | `install.reconcile.reconciled` on success or `install.reconcile.failed` with `reason_code` and `phase` |
| Reconstruction | Every row carries the same `correlation_id`, plus `installation_id` and `desired_hash` |

Unlike `outbound.send` and `host.pair`, the admitted marker is not a separate station. `planned` is the constitutional crossing point: once the desired/applied diff is written, execution is bounded to the planned sub-steps.

---

## 3. v0 manifest contract

The v0 orchestrator intentionally accepts a narrow JSON shape:

```json
{
  "payload_services": [
    { "id": "api", "version": "1" }
  ]
}
```

Rules:

- `payload_services` must be an array.
- every service must have a non-empty string `id`;
- an applied service is converged only when the `id` and full service JSON match;
- `simulate_fail: true` is a test-only mock executor seam for sub-step failure.

---

## 4. Deferred items

- Real bundle installer / host-side executor.
- Business Canon authorization for permitted manifests.
- Elastic Config defaults for rollout strategy, retry policy, and batch sizing.
- Persisting current applied manifest back into `installation_state` from the orchestrator.
- Multi-host fanout; this slice reconciles one installation on one host.
