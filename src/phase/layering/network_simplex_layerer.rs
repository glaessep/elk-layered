//! Port of ELK's `NetworkSimplexLayerer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/NetworkSimplexLayerer.java
//!
//! The network simplex itself is ELK's common one:
//! https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/networksimplex

use std::collections::{HashMap, VecDeque};

use crate::common::networksimplex::{NGraph, NetworkSimplex, NetworkSimplexError};
use crate::graph::{EdgeId, IdVec, LGraph, LayeredEdge, NodeId};

const ITER_LIMIT_FACTOR: i32 = 4;

/// Ports `NetworkSimplexLayerer.process`:
/// `int thoroughness = THOROUGHNESS * ITER_LIMIT_FACTOR;` and
/// `int iterLimit = thoroughness * (int) Math.sqrt(connComp.size());`.
///
/// Java's `int` wraps, and the product can wrap to a negative limit, which stops the loop at
/// once (`iter < iterationLimit` is false). The option value is an `i32`, so the cast is exact.
pub(crate) fn iteration_limit(thoroughness: usize, component_len: usize) -> usize {
    let thoroughness = (thoroughness as i32).wrapping_mul(ITER_LIMIT_FACTOR);
    let limit = thoroughness.wrapping_mul((component_len as f64).sqrt() as i32);
    usize::try_from(limit).unwrap_or(0)
}

pub fn layer_network_simplex(graph: &mut LGraph) -> Result<(), NetworkSimplexError> {
    graph.clear_layers();
    let hidden = graph.hidden_mask();
    let nodes = graph
        .node_ids()
        .filter(|&node| !hidden[node])
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
            graph.set_node_layer(l_node, ngraph.nodes[n_node].layer as usize);
        }

        if connected_components.len() > 1 {
            previous_layering_node_counts = Some(
                graph
                    .layers()
                    .iter()
                    .map(|layer| layer.nodes().len())
                    .collect(),
            );
        }
    }
    Ok(())
}

fn connected_components(graph: &LGraph, nodes: &[NodeId]) -> Vec<Vec<NodeId>> {
    let mut visited = IdVec::filled(graph.layerless_nodes.len(), false);
    let mut components: VecDeque<Vec<NodeId>> = VecDeque::new();

    for node in nodes.iter().copied() {
        if visited[node] {
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
    visited: &mut IdVec<NodeId, bool>,
    component: &mut Vec<NodeId>,
) {
    visited[node] = true;
    component.push(node);

    for edge_index in graph.node_connected_edges(node) {
        let Some(opposite) = opposite_node(graph, node, edge_index) else {
            continue;
        };
        if !visited[opposite] {
            connected_components_dfs(graph, opposite, visited, component);
        }
    }
}

fn initialize(graph: &LGraph, nodes: &[NodeId]) -> NGraph {
    let mut ngraph = NGraph::new();
    let mut node_map = HashMap::new();

    for l_node in nodes {
        let n_node = ngraph.add_node(Some(*l_node));
        node_map.insert(*l_node, n_node);
    }

    for l_node in nodes {
        for edge_index in graph.node_outgoing_edges(*l_node) {
            let edge = &graph.edges[edge_index];
            if edge.source().node == edge.target().node {
                continue;
            }

            let Some(source) = node_map.get(&edge.source().node).copied() else {
                continue;
            };
            let Some(target) = node_map.get(&edge.target().node).copied() else {
                continue;
            };
            ngraph.add_edge(
                Some(edge_index),
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
    if edge.source().node == node {
        Some(edge.target().node)
    } else if edge.target().node == node {
        Some(edge.source().node)
    } else {
        None
    }
}

fn priority_shortness_weight(edge: &LayeredEdge) -> f64 {
    edge.priority_shortness.max(1) as f64
}
