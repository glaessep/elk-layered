//! Port of ELK's `DepthFirstCycleBreaker`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/DepthFirstCycleBreaker.java

use crate::graph::{EdgeId, IdVec, LGraph, NodeId};
use crate::phase::cycles::reverse_feedback_edges;

pub fn break_cycles_depth_first(graph: &mut LGraph) {
    let node_count = graph.layerless_nodes.len();
    let mut visited = IdVec::filled(node_count, false);
    let mut active = IdVec::filled(node_count, false);
    let mut edges_to_reverse = Vec::new();

    let sources = graph
        .node_ids()
        .filter(|node| graph.node_incoming_edges(*node).is_empty())
        .collect::<Vec<_>>();
    for source in sources {
        collect_depth_first_back_edges(
            graph,
            source,
            &mut visited,
            &mut active,
            &mut edges_to_reverse,
        );
    }

    for node in graph.node_ids() {
        if !visited[node] {
            collect_depth_first_back_edges(
                graph,
                node,
                &mut visited,
                &mut active,
                &mut edges_to_reverse,
            );
        }
    }

    reverse_feedback_edges(graph, edges_to_reverse, true);
}

fn collect_depth_first_back_edges(
    graph: &LGraph,
    node: NodeId,
    visited: &mut IdVec<NodeId, bool>,
    active: &mut IdVec<NodeId, bool>,
    edges_to_reverse: &mut Vec<EdgeId>,
) {
    if visited[node] {
        return;
    }

    visited[node] = true;
    active[node] = true;

    for edge_index in graph.node_outgoing_edges(node) {
        let target = graph.edges[edge_index].target().node;
        if target == node {
            continue;
        }
        if active[target] {
            edges_to_reverse.push(edge_index);
        } else {
            collect_depth_first_back_edges(graph, target, visited, active, edges_to_reverse);
        }
    }

    active[node] = false;
}
