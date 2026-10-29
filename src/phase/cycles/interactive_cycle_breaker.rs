//! Port of ELK's `InteractiveCycleBreaker`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/InteractiveCycleBreaker.java

use crate::graph::{EdgeId, IdVec, LGraph, NodeId};
use crate::phase::cycles::reverse_feedback_edges;

pub fn break_cycles_interactive(graph: &mut LGraph) {
    let mut rev_edges = Vec::new();

    for source in graph.node_ids() {
        let source_x = interactive_reference_x(graph, source);
        for edge_index in graph.node_outgoing_edges(source) {
            let target = graph.edges[edge_index].target().node;
            if target == source {
                continue;
            }
            if interactive_reference_x(graph, target) < source_x {
                rev_edges.push(edge_index);
            }
        }
    }

    reverse_feedback_edges(graph, rev_edges, false);

    let node_count = graph.layerless_nodes.len();
    let mut state = IdVec::filled(node_count, InteractiveVisitState::Unvisited);
    let mut rev_edges = Vec::new();
    for node in graph.node_ids() {
        if state[node] == InteractiveVisitState::Unvisited {
            collect_interactive_back_edges(graph, node, &mut state, &mut rev_edges);
        }
    }

    reverse_feedback_edges(graph, rev_edges, false);
}

fn interactive_reference_x(graph: &LGraph, node: NodeId) -> f64 {
    let node = &graph.layerless_nodes[node];
    node.position.x + node.size.width / 2.0
}

fn collect_interactive_back_edges(
    graph: &LGraph,
    node: NodeId,
    state: &mut IdVec<NodeId, InteractiveVisitState>,
    edges_to_reverse: &mut Vec<EdgeId>,
) {
    state[node] = InteractiveVisitState::Active;

    for edge_index in graph.node_outgoing_edges(node) {
        let target = graph.edges[edge_index].target().node;
        if target == node {
            continue;
        }
        match state[target] {
            InteractiveVisitState::Active => edges_to_reverse.push(edge_index),
            InteractiveVisitState::Unvisited => {
                collect_interactive_back_edges(graph, target, state, edges_to_reverse);
            }
            InteractiveVisitState::Visited => {}
        }
    }

    state[node] = InteractiveVisitState::Visited;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractiveVisitState {
    Unvisited,
    Active,
    Visited,
}
