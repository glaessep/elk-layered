//! ELK layered processor pipeline.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/ElkLayered.java
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalNodeResizingProcessor.java

mod assembler;
mod compound;
mod execute;
mod flat;
#[cfg(test)]
mod tests;

use crate::graph::{GraphArena, GraphId, LGraph, LSize, PortSide, is_across_flow};
use crate::intermediate::IntermediateError;
use crate::phase::layering::LayeringError;
use crate::random::RandomSeedError;

pub use assembler::UnportedProcessor;
#[cfg(test)]
pub(crate) use assembler::{IntermediateProcessor, LayeredPhase, Processor};
pub(crate) use compound::{execute_ported_compound_processors, external_port_position};
pub(crate) use flat::execute_ported_processors;
#[cfg(test)]
pub(crate) use flat::{run_processors_until, run_processors_until_phase};

/// Why a layered run stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    /// The split into components or their combination failed.
    #[error(transparent)]
    Component(#[from] crate::components::ComponentError),
    /// A minimum node size is not a finite number.
    #[error(transparent)]
    NodeSize(#[from] crate::options::NodeSizeError),
    /// A network simplex run failed.
    #[error(transparent)]
    NetworkSimplex(#[from] crate::common::networksimplex::NetworkSimplexError),
    /// A graph has the random seed 0.
    #[error(transparent)]
    RandomSeed(#[from] RandomSeedError),
    /// An option asks for a processor of ELK that this crate has not ported.
    #[error("layered processor `{kind:?}` is not ported yet")]
    UnsupportedProcessor {
        /// The processor that is missing.
        kind: UnportedProcessor,
    },
    /// A hierarchy with a feature that the compound run does not support yet.
    #[error("source-backed compound ELK layout does not support this graph yet: {reason}")]
    UnsupportedCompoundGraph {
        /// What the graph has that is not supported.
        reason: &'static str,
    },
    /// An intermediate processor failed.
    #[error(transparent)]
    Intermediate(#[from] IntermediateError),
    /// A layerer failed.
    #[error(transparent)]
    Layering(#[from] LayeringError),
    /// A layer sweep reached `execute_processor`, which has no arena to run it on. The sweep runs
    /// through `execute_in_arena`; reaching this is a bug in the run, never caller input.
    #[error("the layer sweep was run outside the graph arena")]
    SweepOutsideArena,
}

/// The result of a step of a layered run.
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Refuses a graph with an external port dummy that ELK cannot place, or that elk-layered
/// cannot place yet.
///
/// A dummy on a north or south side of a horizontal layout, or on an east or west side of a
/// vertical one, needs the steps for ports across the flow. Issue #24 adds them. Without them the
/// routing of the edges at such a dummy does not match elkjs. A dummy without a side comes from a
/// port without one on a node whose port sides are fixed, and a node of size zero has nothing to
/// take the side from: ELK fails on it.
pub(super) fn check_external_port_sides(graphs: &GraphArena) -> PipelineResult<()> {
    let mut pending = vec![GraphId::ROOT];
    while let Some(id) = pending.pop() {
        let graph = &graphs[id];
        for node in &graph.layerless_nodes {
            let Some(external) = node.external_port() else {
                continue;
            };
            if external.side == PortSide::Undefined {
                return Err(PipelineError::UnsupportedCompoundGraph {
                    reason: "a port of a node with children that has no side, on a node with fixed port sides",
                });
            }
            if is_across_flow(graph.options.direction, external.side) {
                return Err(PipelineError::UnsupportedCompoundGraph {
                    reason: "ports of a node with children on a side across the layout direction, which wait for issue #24",
                });
            }
        }
        pending.extend(graphs.children(id));
    }
    Ok(())
}

pub(super) fn validate_node_size_minima(graphs: &GraphArena) -> PipelineResult<()> {
    let mut pending = vec![GraphId::ROOT];
    while let Some(graph) = pending.pop() {
        graphs[graph].options.validate_node_size_minimum()?;
        pending.extend(graphs.children(graph));
    }
    Ok(())
}

pub(crate) fn actual_graph_size(graph: &LGraph) -> LSize {
    LSize {
        width: graph.size.width + graph.padding.left + graph.padding.right,
        height: graph.size.height + graph.padding.top + graph.padding.bottom,
    }
}
