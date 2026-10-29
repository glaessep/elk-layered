//! Phase 3: crossing minimization by layer sweep, one file per ELK class, and a few helpers
//! the classes share.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order

pub(crate) mod abstract_barycenter_port_distributor;
pub(crate) mod barycenter_heuristic;
pub(crate) mod counting;
pub(crate) mod forster_constraint_resolver;
pub(crate) mod graph_info_holder;
pub(crate) mod greedy_port_distributor;
pub(crate) mod hierarchy_graph_info;
pub(crate) mod layer_sweep_crossing_minimizer;
pub(crate) mod layer_sweep_type_decider;
pub(crate) mod sweep_copy;
pub(crate) mod sweep_port_distributor;
#[cfg(test)]
mod tests;

pub use layer_sweep_crossing_minimizer::CrossMinType;
pub(crate) use layer_sweep_crossing_minimizer::{
    minimize_crossings_layer_sweep_hierarchical_with_type, minimize_crossings_layer_sweep_with_type,
};

use crate::graph::{EdgeId, LGraph, LNodeKind, NodeId, PortRef};

pub(crate) fn connected_edges(graph: &LGraph, port: PortRef) -> impl Iterator<Item = EdgeId> + '_ {
    graph.layerless_nodes[port.node].ports[port.port]
        .incoming_edges()
        .iter()
        .chain(
            graph.layerless_nodes[port.node].ports[port.port]
                .outgoing_edges()
                .iter(),
        )
        .copied()
}

pub(crate) fn is_edge_self_loop(graph: &LGraph, edge: EdgeId) -> bool {
    graph
        .edges
        .get(edge)
        .is_some_and(|edge| edge.source().node == edge.target().node)
}

pub(crate) fn is_in_layer_edge(graph: &LGraph, edge: EdgeId) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    graph.layerless_nodes[edge.source().node].layer_index()
        == graph.layerless_nodes[edge.target().node].layer_index()
}

pub(crate) fn other_end_of_edge(graph: &LGraph, edge: EdgeId, port: PortRef) -> Option<PortRef> {
    let edge = graph.edges.get(edge)?;
    if edge.source() == port {
        Some(edge.target())
    } else if edge.target() == port {
        Some(edge.source())
    } else {
        None
    }
}

pub(crate) fn is_north_south_node(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node].kind() == LNodeKind::NorthSouthPort
}

pub(crate) fn is_long_edge_dummy(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node].kind() == LNodeKind::LongEdge
}

pub(crate) fn first_index(forward: bool, length: usize) -> usize {
    if forward { 0 } else { length - 1 }
}

pub(crate) fn first_free(forward: bool, length: usize) -> isize {
    if forward { 1 } else { length as isize - 2 }
}

pub(crate) fn next_index(index: isize, forward: bool) -> isize {
    if forward { index + 1 } else { index - 1 }
}

pub(crate) fn is_not_end(length: usize, free_layer_index: isize, forward: bool) -> bool {
    if forward {
        free_layer_index < length as isize
    } else {
        free_layer_index >= 0
    }
}

pub(crate) fn is_first_layer(order: &[Vec<NodeId>], current_index: usize, forward: bool) -> bool {
    current_index == first_index(forward, order.len())
}
