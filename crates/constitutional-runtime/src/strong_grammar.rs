//! Strong Grammar v0: deliberative JSON surface → canonical IR graph.
//!
//! Strong Grammar is intentionally not a second command language. It is an
//! ingress/compiler surface for Place-Agent deliberation that emits the same
//! [`IRPrimitive`] graph every other runtime path validates, lowers, plans, and
//! executes. v0 is JSON-only; textual grammar belongs to a later surface pass.

use crate::act_identity::CanonicalActionId;
use crate::ir::{ActionKind, IRPrimitive, InferSurface, Kind, Role, Schema, Window};
use crate::planning_compiler::{CompileError, Edge, IrGraph};
use crate::refs::{DataRef, NodeId, PolicyId, SurfaceRef, TargetRef};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Top-level Strong Grammar program.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum StrongProgram {
    SystemReview {
        #[serde(rename = "SystemReview")]
        system_review: ReviewSpec,
    },
    DriftReview {
        #[serde(rename = "DriftReview")]
        drift_review: ReviewSpec,
    },
    Pipeline {
        #[serde(rename = "Pipeline")]
        pipeline: PipelineSpec,
    },
    Execute {
        #[serde(rename = "Execute")]
        execute: ExecuteSpec,
    },
    Emit {
        #[serde(rename = "Emit")]
        emit: String,
    },
    Confirm {
        #[serde(rename = "Confirm")]
        confirm: ConfirmSpec,
    },
}

/// Review-shaped program used by the Agent Runtime for bounded deliberation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ReviewSpec {
    pub target: String,
    #[serde(default)]
    pub pipeline: Vec<PipelineStep>,
    #[serde(default)]
    pub on_success: Option<StrongHandler>,
    #[serde(default)]
    pub on_failure: Option<StrongHandler>,
}

/// Generic pipeline form for v0 JSON programs that are not named reviews.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PipelineSpec {
    pub target: String,
    pub steps: Vec<PipelineStep>,
    #[serde(default)]
    pub on_success: Option<StrongHandler>,
    #[serde(default)]
    pub on_failure: Option<StrongHandler>,
}

/// Pipeline step names are closed in v0. Unknown strings are compile errors,
/// not custom commands.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub enum PipelineStep {
    Collect,
    Compress,
    Classify,
    Prioritize,
    Compare,
}

impl TryFrom<String> for PipelineStep {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.trim().to_ascii_lowercase().as_str() {
            "collect" => Ok(Self::Collect),
            "compress" => Ok(Self::Compress),
            "classify" => Ok(Self::Classify),
            "prioritize" => Ok(Self::Prioritize),
            "compare" => Ok(Self::Compare),
            other => Err(format!(
                "unsupported Strong pipeline step `{other}`; v0 allows Collect, Compress, Classify, Prioritize, Compare"
            )),
        }
    }
}

/// Branch handler for a pipeline/review. v0 deliberately limits branch effects
/// to primitives that already exist in the IR.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum StrongHandler {
    Emit {
        #[serde(rename = "Emit")]
        emit: String,
    },
    Execute {
        #[serde(rename = "Execute")]
        execute: ExecuteSpec,
    },
    Confirm {
        #[serde(rename = "Confirm")]
        confirm: ConfirmSpec,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ExecuteSpec {
    pub action: String,
    #[serde(default)]
    pub params: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ConfirmSpec {
    pub role: String,
    pub action: ExecuteSpec,
}

/// Parse a v0 JSON Strong Grammar program.
pub fn parse_strong_json(input: &str) -> Result<StrongProgram, CompileError> {
    serde_json::from_str(input).map_err(|err| CompileError::StrongSyntaxError(err.to_string()))
}

/// Compile a parsed Strong Grammar program into a deterministic IR graph.
pub fn compile_strong_program(program: &StrongProgram) -> Result<IrGraph, CompileError> {
    StrongCompiler::default().compile(program)
}

/// Parse and compile a v0 JSON Strong Grammar program into a deterministic IR graph.
pub fn compile_strong_json_to_ir_graph(input: &str) -> Result<IrGraph, CompileError> {
    let program = parse_strong_json(input)?;
    compile_strong_program(&program)
}

#[derive(Default)]
struct StrongCompiler {
    graph: IrGraph,
    next_id: usize,
}

impl StrongCompiler {
    fn compile(mut self, program: &StrongProgram) -> Result<IrGraph, CompileError> {
        match program {
            StrongProgram::SystemReview { system_review } => self.compile_review(
                "system.review",
                system_review,
                default_review_steps(&system_review.pipeline),
            )?,
            StrongProgram::DriftReview { drift_review } => self.compile_review(
                "drift.review",
                drift_review,
                default_review_steps(&drift_review.pipeline),
            )?,
            StrongProgram::Pipeline { pipeline } => self.compile_pipeline(
                "pipeline",
                &pipeline.target,
                &pipeline.steps,
                pipeline.on_success.as_ref(),
                pipeline.on_failure.as_ref(),
            )?,
            StrongProgram::Execute { execute } => {
                self.push_node(execute_primitive(execute)?, None);
            }
            StrongProgram::Emit { emit } => {
                self.push_node(
                    IRPrimitive::Emit {
                        surface: SurfaceRef(emit.clone()),
                        payload: DataRef("strong.input".into()),
                    },
                    None,
                );
            }
            StrongProgram::Confirm { confirm } => {
                self.push_node(confirm_primitive(confirm)?, None);
            }
        }
        Ok(self.graph)
    }

    fn compile_review(
        &mut self,
        family: &str,
        review: &ReviewSpec,
        steps: Vec<PipelineStep>,
    ) -> Result<(), CompileError> {
        self.compile_pipeline(
            family,
            &review.target,
            &steps,
            review.on_success.as_ref(),
            review.on_failure.as_ref(),
        )
    }

    fn compile_pipeline(
        &mut self,
        family: &str,
        target: &str,
        steps: &[PipelineStep],
        on_success: Option<&StrongHandler>,
        on_failure: Option<&StrongHandler>,
    ) -> Result<(), CompileError> {
        if target.trim().is_empty() {
            return Err(CompileError::StrongLoweringError(
                "Strong pipeline target must not be empty".into(),
            ));
        }
        if steps.is_empty() {
            return Err(CompileError::StrongLoweringError(
                "Strong pipeline must contain at least one step".into(),
            ));
        }

        let target_ref = TargetRef(target.to_owned());
        let infer_surface = infer_surface_for_target(target);
        let mut previous: Option<NodeId> = None;

        for step in steps {
            let primitive =
                step_to_primitive(family, step, &target_ref, previous.as_ref(), &infer_surface)?;
            let id = self.push_node(primitive, previous.as_ref());
            previous = Some(id);
        }

        if let Some(handler) = on_success {
            self.compile_handler("on_success", handler, previous.as_ref())?;
        }
        if let Some(handler) = on_failure {
            self.compile_handler("on_failure", handler, previous.as_ref())?;
        }
        Ok(())
    }

    fn compile_handler(
        &mut self,
        branch: &str,
        handler: &StrongHandler,
        parent: Option<&NodeId>,
    ) -> Result<NodeId, CompileError> {
        let primitive = match handler {
            StrongHandler::Emit { emit } => IRPrimitive::Route {
                operation: Box::new(IRPrimitive::Emit {
                    surface: SurfaceRef(emit.clone()),
                    payload: DataRef(parent_ref(parent)),
                }),
                surface: SurfaceRef(emit.clone()),
            },
            StrongHandler::Execute { execute } => IRPrimitive::Route {
                operation: Box::new(execute_primitive(execute)?),
                surface: SurfaceRef(branch.into()),
            },
            StrongHandler::Confirm { confirm } => IRPrimitive::Route {
                operation: Box::new(confirm_primitive(confirm)?),
                surface: SurfaceRef(branch.into()),
            },
        };
        Ok(self.push_node(primitive, parent))
    }

    fn push_node(&mut self, body: IRPrimitive, parent: Option<&NodeId>) -> NodeId {
        let id = NodeId(format!("strong.n{}", self.next_id));
        self.next_id += 1;
        if let Some(parent) = parent {
            self.graph.edges.push(Edge {
                parent: parent.clone(),
                child: id.clone(),
            });
        }
        self.graph.nodes.push(crate::ir::IrNode {
            id: id.clone(),
            body,
        });
        id
    }
}

fn default_review_steps(configured: &[PipelineStep]) -> Vec<PipelineStep> {
    if configured.is_empty() {
        vec![
            PipelineStep::Collect,
            PipelineStep::Compress,
            PipelineStep::Classify,
            PipelineStep::Prioritize,
        ]
    } else {
        configured.to_vec()
    }
}

fn step_to_primitive(
    family: &str,
    step: &PipelineStep,
    target: &TargetRef,
    previous: Option<&NodeId>,
    infer_surface: &InferSurface,
) -> Result<IRPrimitive, CompileError> {
    let input_ref = || DataRef(parent_ref(previous));
    Ok(match step {
        PipelineStep::Collect => IRPrimitive::Collect {
            kind: Kind(format!("{family}.events")),
            target: target.clone(),
            window: Window("latest".into()),
        },
        PipelineStep::Compress => IRPrimitive::Compress {
            kind: Kind(format!("{family}.summary")),
            input_ref: input_ref(),
            infer_surface: infer_surface.clone(),
        },
        PipelineStep::Classify => IRPrimitive::Classify {
            kind: Kind(format!("{family}.classification")),
            input_ref: input_ref(),
            schema: Schema(family.into()),
            infer_surface: infer_surface.clone(),
        },
        PipelineStep::Prioritize => IRPrimitive::Prioritize {
            kind: Kind(format!("{family}.priority")),
            input_ref: input_ref(),
            policy: PolicyId(format!("{family}.priority")),
            infer_surface: infer_surface.clone(),
        },
        PipelineStep::Compare => IRPrimitive::Compare {
            kind: Kind(format!("{family}.drift")),
            left: input_ref(),
            right: DataRef(format!("{family}.baseline")),
        },
    })
}

fn execute_primitive(spec: &ExecuteSpec) -> Result<IRPrimitive, CompileError> {
    let action = CanonicalActionId::parse(&spec.action).map_err(|err| {
        CompileError::StrongLoweringError(format!(
            "Strong Execute action must be canonical namespace.verb: {err}"
        ))
    })?;
    Ok(IRPrimitive::Execute {
        action: ActionKind::Canonical(action),
        params: spec.params.clone(),
    })
}

fn confirm_primitive(spec: &ConfirmSpec) -> Result<IRPrimitive, CompileError> {
    if spec.role.trim().is_empty() {
        return Err(CompileError::StrongLoweringError(
            "Strong Confirm role must not be empty".into(),
        ));
    }
    Ok(IRPrimitive::Confirm {
        action: Box::new(execute_primitive(&spec.action)?),
        role: Role(spec.role.clone()),
    })
}

fn parent_ref(parent: Option<&NodeId>) -> String {
    parent
        .map(|id| id.0.clone())
        .unwrap_or_else(|| "strong.input".into())
}

fn infer_surface_for_target(target: &str) -> InferSurface {
    match target.trim().to_ascii_lowercase().as_str() {
        "lab256" => InferSurface::Lab256,
        "lab8gb" => InferSurface::Lab8gb,
        "lab512" => InferSurface::Lab512,
        "cloud" => InferSurface::Cloud,
        "hybrid" => InferSurface::Hybrid,
        _ => InferSurface::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IrNode;
    use crate::validation::{validate_structure, AdmissibilityContext};

    #[test]
    fn system_review_json_compiles_to_pipeline_and_success_route() {
        let graph = compile_strong_json_to_ir_graph(
            r#"{
              "SystemReview": {
                "target": "lab8gb",
                "pipeline": ["Collect", "Compress", "Classify", "Prioritize"],
                "on_success": { "Emit": "chat.reply" }
              }
            }"#,
        )
        .expect("valid Strong JSON should compile");

        assert_eq!(graph.nodes.len(), 5);
        assert_eq!(graph.edges.len(), 4);
        assert!(matches!(graph.nodes[0].body, IRPrimitive::Collect { .. }));
        assert!(matches!(graph.nodes[1].body, IRPrimitive::Compress { .. }));
        assert!(matches!(graph.nodes[2].body, IRPrimitive::Classify { .. }));
        assert!(matches!(
            graph.nodes[3].body,
            IRPrimitive::Prioritize { .. }
        ));
        assert!(matches!(graph.nodes[4].body, IRPrimitive::Route { .. }));

        for node in &graph.nodes {
            validate_structure(node).expect("Strong compiler must emit structurally valid IR");
        }
    }

    #[test]
    fn drift_review_on_failure_compiles_to_route_node() {
        let graph = compile_strong_json_to_ir_graph(
            r#"{
              "DriftReview": {
                "target": "lab512",
                "pipeline": ["Collect", "Compare"],
                "on_failure": { "Execute": { "action": "install.reconcile", "params": {"host_id": "h"} } }
              }
            }"#,
        )
        .expect("valid DriftReview should compile");

        assert_eq!(graph.nodes.len(), 3);
        assert!(matches!(graph.nodes[1].body, IRPrimitive::Compare { .. }));
        match &graph.nodes[2].body {
            IRPrimitive::Route { surface, operation } => {
                assert_eq!(surface.0, "on_failure");
                assert!(matches!(**operation, IRPrimitive::Execute { .. }));
            }
            other => panic!("expected Route, got {other:?}"),
        }
    }

    #[test]
    fn execute_requires_canonical_action_identity() {
        let err = compile_strong_json_to_ir_graph(
            r#"{ "Execute": { "action": "not_canonical", "params": {} } }"#,
        )
        .unwrap_err();
        assert!(matches!(err, CompileError::StrongLoweringError(_)));
    }

    #[test]
    fn confirm_compiles_to_confirmed_execute() {
        let graph = compile_strong_json_to_ir_graph(
            r#"{
              "Confirm": {
                "role": "Founder",
                "action": { "action": "host.pair", "params": {"host_id": "h"} }
              }
            }"#,
        )
        .expect("Confirm should compile");

        match &graph.nodes[0].body {
            IRPrimitive::Confirm { role, action } => {
                assert_eq!(role.0, "Founder");
                assert!(matches!(**action, IRPrimitive::Execute { .. }));
            }
            other => panic!("expected Confirm, got {other:?}"),
        }
    }

    #[test]
    fn default_system_review_pipeline_is_structurally_admissible_shape() {
        let graph =
            compile_strong_json_to_ir_graph(r#"{ "SystemReview": { "target": "lab8gb" } }"#)
                .expect("default review should compile");
        let ctx = AdmissibilityContext::default();
        for IrNode { body, id } in graph.nodes {
            let node = IrNode { id, body };
            validate_structure(&node).unwrap();
            assert!(ctx.require_evidence_closure);
        }
    }
}
