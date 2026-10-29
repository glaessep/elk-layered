//! Phase 2 layering processors.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/NetworkSimplexLayerer.java
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/networksimplex

use std::collections::{HashMap, VecDeque};

use crate::common::networksimplex::{NGraph, NetworkSimplex, NetworkSimplexError};
use crate::graph::{EdgeId, LGraph, LayeredEdge, NodeId};

mod coffman_graham;
#[cfg(test)]
mod elkjs_0_9_3_cases;
#[cfg(test)]
mod execution_tests;
mod interactive;
mod longest_path;
mod min_width;
mod stretch_width;

use crate::options::LayeringStrategy;

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
    incoming: Vec<Vec<EdgeId>>,
    outgoing: Vec<Vec<EdgeId>>,
}

impl<'a> LayeringInput<'a> {
    fn new(graph: &'a LGraph) -> Self {
        let nodes = graph
            .node_ids()
            .filter(|&node| !graph.layerless_nodes[node].hidden)
            .collect();
        let incoming = graph
            .node_ids()
            .map(|node| {
                graph
                    .node_incoming_edges(node)
                    .into_iter()
                    .filter(|&edge| !graph.layerless_nodes[graph.edges[edge].source.node].hidden)
                    .collect()
            })
            .collect();
        let outgoing = graph
            .node_ids()
            .map(|node| {
                graph
                    .node_outgoing_edges(node)
                    .into_iter()
                    .filter(|&edge| !graph.layerless_nodes[graph.edges[edge].target.node].hidden)
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
        self.graph.edges[edge].source.node
    }
    fn target(&self, edge: EdgeId) -> NodeId {
        self.graph.edges[edge].target.node
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
        let index = graph.layers.len();
        for &node in &nodes {
            graph.layerless_nodes[node].layer_index = Some(index);
        }
        let mut layer = crate::graph::Layer::new();
        layer.nodes = nodes;
        graph.layers.push(layer);
    }
    Ok(())
}

const ITER_LIMIT_FACTOR: i32 = 4;

/// Ports `NetworkSimplexLayerer.process`:
/// `int thoroughness = THOROUGHNESS * ITER_LIMIT_FACTOR;` and
/// `int iterLimit = thoroughness * (int) Math.sqrt(connComp.size());`.
///
/// Java's `int` wraps, and the product can wrap to a negative limit, which stops the loop at
/// once (`iter < iterationLimit` is false). The option value is an `i32`, so the cast is exact.
fn iteration_limit(thoroughness: usize, component_len: usize) -> usize {
    let thoroughness = (thoroughness as i32).wrapping_mul(ITER_LIMIT_FACTOR);
    let limit = thoroughness.wrapping_mul((component_len as f64).sqrt() as i32);
    usize::try_from(limit).unwrap_or(0)
}

pub fn layer_network_simplex(graph: &mut LGraph) -> Result<(), NetworkSimplexError> {
    graph.clear_layers();
    let nodes = graph
        .node_ids()
        .filter(|&node| !graph.layerless_nodes[node].hidden)
        .collect::<Vec<_>>();
    if nodes.is_empty() {
        return Ok(());
    }

    let connected_components = connected_components(graph, &nodes);
    let mut previous_layering_node_counts = None;

    for component in connected_components.iter() {
        let iter_limit = iteration_limit(graph.options.thoroughness, component.len());
        let mut ngraph = initialize(graph, component);

        NetworkSimplex::for_graph(&mut ngraph)
            .with_iteration_limit(iter_limit)
            .with_previous_layering(previous_layering_node_counts.clone())
            .with_balancing(true)
            .execute()?;

        for n_node in ngraph.active_nodes().iter().copied() {
            let Some(l_node) = ngraph.nodes[n_node].origin else {
                continue;
            };
            graph.set_node_layer(NodeId::new(l_node), ngraph.nodes[n_node].layer as usize);
        }

        if connected_components.len() > 1 {
            previous_layering_node_counts =
                Some(graph.layers.iter().map(|layer| layer.nodes.len()).collect());
        }
    }
    Ok(())
}

fn connected_components(graph: &LGraph, nodes: &[NodeId]) -> Vec<Vec<NodeId>> {
    let mut visited = vec![false; graph.layerless_nodes.len()];
    let mut components: VecDeque<Vec<NodeId>> = VecDeque::new();

    for node in nodes.iter().copied() {
        if visited[node.index()] {
            continue;
        }

        let mut component = Vec::new();
        connected_components_dfs(graph, node, &mut visited, &mut component);
        if components
            .front()
            .map(|front| front.len() < component.len())
            .unwrap_or(true)
        {
            components.push_front(component);
        } else {
            components.push_back(component);
        }
    }

    components.into_iter().collect()
}

fn connected_components_dfs(
    graph: &LGraph,
    node: NodeId,
    visited: &mut [bool],
    component: &mut Vec<NodeId>,
) {
    visited[node.index()] = true;
    component.push(node);

    for edge_index in graph.node_connected_edges(node) {
        let Some(opposite) = opposite_node(graph, node, edge_index) else {
            continue;
        };
        if !visited[opposite.index()] {
            connected_components_dfs(graph, opposite, visited, component);
        }
    }
}

fn initialize(graph: &LGraph, nodes: &[NodeId]) -> NGraph {
    let mut ngraph = NGraph::new();
    let mut node_map = HashMap::new();

    for l_node in nodes {
        let n_node = ngraph.add_node(Some(l_node.index()));
        node_map.insert(*l_node, n_node);
    }

    for l_node in nodes {
        for edge_index in graph.node_outgoing_edges(*l_node) {
            let edge = &graph.edges[edge_index];
            if edge.source.node == edge.target.node {
                continue;
            }

            let Some(source) = node_map.get(&edge.source.node).copied() else {
                continue;
            };
            let Some(target) = node_map.get(&edge.target.node).copied() else {
                continue;
            };
            ngraph.add_edge(
                Some(edge_index.index()),
                source,
                target,
                priority_shortness_weight(edge),
                1,
            );
        }
    }

    ngraph
}

fn opposite_node(graph: &LGraph, node: NodeId, edge_index: EdgeId) -> Option<NodeId> {
    let edge = graph.edges.get(edge_index)?;
    if edge.source.node == node {
        Some(edge.target.node)
    } else if edge.target.node == node {
        Some(edge.source.node)
    } else {
        None
    }
}

fn priority_shortness_weight(edge: &LayeredEdge) -> f64 {
    edge.priority_shortness.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::options::{ElkDirection, LayeredOptions};
    use crate::p1cycles::break_cycles_greedy;

    // Mutation: multiply in `usize` instead of wrapping in `i32`.
    #[test]
    fn the_iteration_limit_wraps_like_javas_int() {
        // 7 * 4 * (int) sqrt(10) = 84.
        assert_eq!(iteration_limit(7, 10), 84);
        // 2^29 * 4 wraps to i32::MIN, and the limit is then negative: no iterations.
        assert_eq!(iteration_limit(1 << 29, 10), 0);
        // (2^30 + 5) * 4 wraps to 20.
        assert_eq!(iteration_limit((1 << 30) + 5, 1), 20);
    }

    pub(super) fn node(id: &str) -> ElkInputNode {
        ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: id.to_string(),
            width: 80.0,
            height: 40.0,
            parent: None,
            direction: None,
            hierarchy_handling: None,
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::FIXED,
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: None,
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
        }
    }

    pub(super) fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
        ElkInputEdge {
            source_port: None,
            target_port: None,
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            labels: Vec::new(),
            minlen: 1,
            inside_self_loops_yo: false,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            container: None,
        }
    }

    pub(super) fn graph(
        nodes: Vec<ElkInputNode>,
        edges: Vec<ElkInputEdge>,
    ) -> crate::graph::GraphArena {
        import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap()
    }

    #[test]
    fn network_simplex_layerer_assigns_forward_layers() {
        let mut graph = graph(
            vec![node("A"), node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
        );

        layer_network_simplex(&mut graph).unwrap();

        assert_eq!(graph.layers.len(), 3);
        assert_layer_order(&graph, "A", "B");
        assert_layer_order(&graph, "B", "C");
    }

    #[test]
    fn network_simplex_layerer_ignores_self_loops() {
        let mut graph = graph(
            vec![node("A"), node("B")],
            vec![edge("A-A", "A", "A"), edge("A-B", "A", "B")],
        );

        layer_network_simplex(&mut graph).unwrap();

        assert_layer_order(&graph, "A", "B");
    }

    #[test]
    fn network_simplex_layerer_processes_connected_components() {
        let mut graph = graph(
            vec![node("A"), node("B"), node("C"), node("D")],
            vec![edge("A-B", "A", "B"), edge("C-D", "C", "D")],
        );

        layer_network_simplex(&mut graph).unwrap();

        assert_layer_order(&graph, "A", "B");
        assert_layer_order(&graph, "C", "D");
        assert_eq!(graph.layers[0].nodes.len(), 2);
    }

    #[test]
    fn greedy_cycle_breaker_output_can_be_layered_by_network_simplex() {
        let mut graph = graph(
            vec![node("A"), node("B"), node("C")],
            vec![
                edge("A-B", "A", "B"),
                edge("B-C", "B", "C"),
                edge("C-A", "C", "A"),
            ],
        );

        break_cycles_greedy(&mut graph);
        layer_network_simplex(&mut graph).unwrap();

        for edge in &graph.edges {
            if edge.source.node == edge.target.node {
                continue;
            }
            let source_layer = graph.layerless_nodes[edge.source.node].layer_index.unwrap();
            let target_layer = graph.layerless_nodes[edge.target.node].layer_index.unwrap();
            assert!(
                target_layer > source_layer,
                "edge {} should point from an earlier layer to a later layer",
                edge.id
            );
        }
    }

    fn assert_layer_order(graph: &LGraph, source: &str, target: &str) {
        let source = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == source)
            .unwrap();
        let target = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == target)
            .unwrap();
        assert!(target.layer_index.unwrap() > source.layer_index.unwrap());
    }
}
