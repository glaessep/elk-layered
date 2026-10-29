//! Phase 2: layering, one file per ELK class, and the strategy switch that picks one.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers

use crate::graph::{EdgeId, IdVec, LGraph, NodeId};
use crate::options::LayeringStrategy;

mod coffman_graham;
#[cfg(test)]
mod elkjs_0_9_3_cases;
#[cfg(test)]
mod execution_tests;
mod interactive;
mod longest_path;
mod min_width;
pub(crate) mod network_simplex_layerer;
mod stretch_width;
#[cfg(test)]
mod tests;

pub use network_simplex_layerer::layer_network_simplex;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayeringError {
    #[error("layer assignment requires an acyclic graph")]
    CyclicGraph,
    #[error("stretch-width layering cannot normalize these node dimensions")]
    InvalidStretchWidthDimensions,
    #[error("stretch-width layering needs more than {limit} retries to fit the widest node")]
    StretchWidthRetryLimit {
        /// The retry bound that was reached.
        limit: usize,
    },
}

type LayeringResult<T> = Result<T, LayeringError>;

/// Snapshot adjacency in ELK port order; hidden constraint nodes are not layerless input.
struct LayeringInput<'a> {
    graph: &'a LGraph,
    nodes: Vec<NodeId>,
    incoming: IdVec<NodeId, Vec<EdgeId>>,
    outgoing: IdVec<NodeId, Vec<EdgeId>>,
}

impl<'a> LayeringInput<'a> {
    fn new(graph: &'a LGraph) -> Self {
        let hidden = graph.hidden_mask();
        let nodes = graph.node_ids().filter(|&node| !hidden[node]).collect();
        let incoming = graph
            .node_ids()
            .map(|node| {
                graph
                    .node_incoming_edges(node)
                    .into_iter()
                    .filter(|&edge| !hidden[graph.edges[edge].source().node])
                    .collect()
            })
            .collect();
        let outgoing = graph
            .node_ids()
            .map(|node| {
                graph
                    .node_outgoing_edges(node)
                    .into_iter()
                    .filter(|&edge| !hidden[graph.edges[edge].target().node])
                    .collect()
            })
            .collect();
        Self {
            graph,
            nodes,
            incoming,
            outgoing,
        }
    }

    fn source(&self, edge: EdgeId) -> NodeId {
        self.graph.edges[edge].source().node
    }
    fn target(&self, edge: EdgeId) -> NodeId {
        self.graph.edges[edge].target().node
    }
    fn len(&self) -> usize {
        self.graph.layerless_nodes.len()
    }
}

/// Compute the layers in private state. Only a completed layering mutates the graph, so an error
/// never leaves half-assigned layers.
pub(crate) fn layer_with_strategy(
    graph: &mut LGraph,
    strategy: LayeringStrategy,
) -> LayeringResult<()> {
    let input = LayeringInput::new(graph);
    let layers = match strategy {
        LayeringStrategy::LongestPath => longest_path::layer(&input, false)?,
        LayeringStrategy::LongestPathSource => longest_path::layer(&input, true)?,
        LayeringStrategy::CoffmanGraham => coffman_graham::layer(&input)?,
        LayeringStrategy::Interactive => interactive::layer(&input)?,
        LayeringStrategy::MinWidth => min_width::layer(&input)?,
        LayeringStrategy::StretchWidth => stretch_width::layer(&input)?,
        _ => unreachable!("the pipeline dispatches only the six source-backed layerers here"),
    };
    graph.clear_layers();
    for nodes in layers {
        let index = graph.layers().len();
        graph.set_layer_nodes(index, nodes);
    }
    Ok(())
}
