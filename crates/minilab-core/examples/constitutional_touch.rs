//! One `Decide` → compile → lower, using `constitutional-runtime` next to `minilab-core`.
//!
//! Run: `cargo run -p minilab-core --example constitutional_touch`

use constitutional_runtime::decision::{
    compile_flow, resolve_lower_one, DecideResolver, PlannerError,
};
use constitutional_runtime::lowering::MinilabRuntimeLowerer;
use constitutional_runtime::refs::{NodeId, TargetRef};
use constitutional_runtime::{DataRef, IRPrimitive, IrNode, PolicyId};

struct AlwaysObserve;

impl DecideResolver for AlwaysObserve {
    fn resolve_decide(&self, _node: &IrNode) -> Result<IRPrimitive, PlannerError> {
        Ok(IRPrimitive::Observe {
            target: TargetRef("lab8gb".into()),
            scope: "gtm.signals".into(),
        })
    }
}

fn main() {
    let node = IrNode {
        id: NodeId("decide-1".into()),
        body: IRPrimitive::Decide {
            context: DataRef("minilab/week1".into()),
            policy: PolicyId("policy/gtm".into()),
        },
    };
    let resolver = AlwaysObserve;
    let compiled = compile_flow(std::slice::from_ref(&node), &resolver).expect("compile");
    assert!(matches!(compiled[0].body, IRPrimitive::Observe { .. }));

    let lowerer = MinilabRuntimeLowerer;
    let (_plan, cmd) = resolve_lower_one(&lowerer, &node, &resolver).expect("lower");
    println!("Operational command: {}.{}", cmd.namespace, cmd.verb);
    println!("minilab-core + constitutional-runtime: OK");
}
