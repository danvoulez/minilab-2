//! End-to-end: admissibility → lower → evidence closure semantics.

use constitutional_runtime::decision::{
    compile_flow, resolve_lower_one, DecideResolver, PlannerError,
};
use constitutional_runtime::evidence::{
    close_execution_evidence, EvidenceRecord, FailureToClose, FileEvidenceStore,
};
use constitutional_runtime::lowering::{Lowerer, MinilabRuntimeLowerer};
use constitutional_runtime::refs::{NodeId, TargetRef};
use constitutional_runtime::validation::{
    validate_admissibility, AdmissibilityContext, AdmissibleNode,
};
use constitutional_runtime::{
    CapabilityManifest, DataRef, IRPrimitive, IrNode, Kind, PolicyClass, PolicyId, PrimitiveName,
    Window,
};
use std::collections::BTreeSet;
use tempfile::tempdir;

struct ResolveToObserve;

impl DecideResolver for ResolveToObserve {
    fn resolve_decide(&self, _node: &IrNode) -> Result<IRPrimitive, PlannerError> {
        Ok(IRPrimitive::Observe {
            target: TargetRef("lab8gb".into()),
            scope: "facts".into(),
        })
    }
}

#[test]
fn decide_layer_then_runtime_lower() {
    let n = IrNode {
        id: NodeId("dec1".into()),
        body: IRPrimitive::Decide {
            context: DataRef("ctx".into()),
            policy: PolicyId("policy/main".into()),
        },
    };
    let compiled = compile_flow(&[n.clone()], &ResolveToObserve).unwrap();
    assert!(matches!(compiled[0].body, IRPrimitive::Observe { .. }));
    let lowerer = MinilabRuntimeLowerer;
    let (_, cmd) = resolve_lower_one(&lowerer, &n, &ResolveToObserve).unwrap();
    assert_eq!(cmd.namespace, "host");
}

fn full_manifest() -> CapabilityManifest {
    CapabilityManifest {
        substrate_id: "test".into(),
        substrate_version: "1".into(),
        supported_primitives: BTreeSet::from_iter([
            PrimitiveName::Observe,
            PrimitiveName::Collect,
            PrimitiveName::Fetch,
            PrimitiveName::Compress,
            PrimitiveName::Classify,
            PrimitiveName::Prioritize,
            PrimitiveName::Compare,
            PrimitiveName::Emit,
            PrimitiveName::Execute,
            PrimitiveName::Route,
            PrimitiveName::Schedule,
            PrimitiveName::Persist,
            PrimitiveName::Confirm,
            PrimitiveName::Cancel,
            PrimitiveName::Reconcile,
        ]),
        declared_guarantees: BTreeSet::from(["evidence.write".into()]),
        ..Default::default()
    }
}

#[test]
fn intelligence_loop_admit_lower_evidence() {
    let node = IrNode {
        id: NodeId("loop-1".into()),
        body: IRPrimitive::Collect {
            kind: Kind("events".into()),
            target: TargetRef("lab8gb".into()),
            window: Window("24h".into()),
        },
    };
    let ctx = AdmissibilityContext::default();
    let AdmissibleNode { node: adm } =
        validate_admissibility(&node, &[full_manifest()], &ctx).expect("admissible");

    let lowerer = MinilabRuntimeLowerer;
    let (plan, cmd) = lowerer.lower(&adm).expect("lower");

    let dir = tempdir().unwrap();
    let path = dir.path().join("evidence.ndjson");
    let store = FileEvidenceStore::new(&path);

    let records = vec![
        EvidenceRecord::from_plan(&adm.id, &plan, &cmd),
        EvidenceRecord::from_execution_result(&adm.id, true, Some("out:1")),
    ];
    close_execution_evidence(&store, records).expect("closure");

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("loop-1"));
    assert!(text.contains("events"));
}

#[test]
fn success_without_evidence_write_fails_close() {
    struct FailingStore;
    impl constitutional_runtime::EvidenceStore for FailingStore {
        fn write_record(
            &self,
            _record: constitutional_runtime::EvidenceRecord,
        ) -> Result<(), constitutional_runtime::EvidenceStoreError> {
            Err(constitutional_runtime::EvidenceStoreError::Serde(
                "disk full".into(),
            ))
        }
    }

    let r = close_execution_evidence(
        &FailingStore,
        vec![EvidenceRecord {
            kind: "x".into(),
            payload_json: serde_json::json!({}),
        }],
    );
    assert!(matches!(r, Err(FailureToClose::EvidenceWriteFailed(_))));
}

#[test]
fn policy_class_d_rejected_at_runtime_boundary() {
    let node = IrNode {
        id: NodeId("d1".into()),
        body: IRPrimitive::Collect {
            kind: Kind("events".into()),
            target: TargetRef("lab8gb".into()),
            window: Window("24h".into()),
        },
    };
    let ctx = AdmissibilityContext {
        policy_class: PolicyClass::D,
        at_execution_boundary: true,
        ..Default::default()
    };
    assert!(validate_admissibility(&node, &[full_manifest()], &ctx).is_err());
}

#[test]
fn policy_blocked_node() {
    let node = IrNode {
        id: NodeId("x".into()),
        body: IRPrimitive::Execute {
            action: constitutional_runtime::ir::ActionKind::Canonical(
                constitutional_runtime::CanonicalActionId::new("x", "y").unwrap(),
            ),
            params: Default::default(),
        },
    };
    let ctx = AdmissibilityContext {
        policy_class: PolicyClass::A,
        ..Default::default()
    };
    assert!(validate_admissibility(&node, &[full_manifest()], &ctx).is_err());
}

#[test]
fn all_intelligence_primitives_render() {
    let lowerer = MinilabRuntimeLowerer;
    let id = NodeId("n".into());
    let cases: Vec<IRPrimitive> = vec![
        IRPrimitive::Observe {
            target: TargetRef("h".into()),
            scope: "s".into(),
        },
        IRPrimitive::Collect {
            kind: Kind("k".into()),
            target: TargetRef("h".into()),
            window: Window("1h".into()),
        },
        IRPrimitive::Fetch {
            kind: Kind("k".into()),
            id: "id1".into(),
        },
        IRPrimitive::Compress {
            kind: Kind("k".into()),
            input_ref: constitutional_runtime::refs::DataRef("r".into()),
            infer_surface: constitutional_runtime::ir::InferSurface::Local,
        },
        IRPrimitive::Classify {
            kind: Kind("k".into()),
            input_ref: constitutional_runtime::refs::DataRef("r".into()),
            schema: constitutional_runtime::ir::Schema("s".into()),
            infer_surface: constitutional_runtime::ir::InferSurface::Local,
        },
        IRPrimitive::Prioritize {
            kind: Kind("k".into()),
            input_ref: constitutional_runtime::refs::DataRef("r".into()),
            policy: constitutional_runtime::refs::PolicyId("p".into()),
            infer_surface: constitutional_runtime::ir::InferSurface::Local,
        },
        IRPrimitive::Compare {
            kind: Kind("k".into()),
            left: constitutional_runtime::refs::DataRef("a".into()),
            right: constitutional_runtime::refs::DataRef("b".into()),
        },
    ];
    for body in cases {
        let n = IrNode {
            id: id.clone(),
            body,
        };
        lowerer.lower(&n).expect("expected lowering");
    }
}
