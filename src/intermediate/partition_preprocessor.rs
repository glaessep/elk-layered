//! Port of ELK's `PartitionPreprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PartitionPreprocessor.java

use crate::graph::{EdgeId, LGraph, reverse_edge};

/// `PARTITION_CONSTRAINT_EDGE_PRIORITY`: the direction priority that a reversed edge gets on top
/// of the priority the input sets.
const PARTITION_CONSTRAINT_EDGE_PRIORITY: i32 = 1_000;

/// Reverses each edge that runs from a higher partition to a lower one, so that the layering
/// never has to place the higher partition first.
///
/// Java: `PartitionPreprocessor.process`. It collects the edges first and reverses them after,
/// because a reversal moves an edge to another node's list of outgoing edges.
pub fn preprocess_partitions(graph: &mut LGraph) {
    let mut to_reverse = Vec::new();
    for node in graph.node_ids() {
        if graph.layerless_nodes[node].partition.is_none() {
            continue;
        }
        for edge in graph.node_outgoing_edges(node) {
            if must_be_reversed(graph, edge) {
                to_reverse.push(edge);
            }
        }
    }
    for edge in to_reverse {
        reverse(graph, edge);
    }
}

/// Java: `PartitionPreprocessor.mustBeReversed`. An edge into a node with no partition stays.
fn must_be_reversed(graph: &LGraph, edge: EdgeId) -> bool {
    let edge = &graph.edges[edge];
    let source = graph.layerless_nodes[edge.source().node].partition;
    let target = graph.layerless_nodes[edge.target().node].partition;
    matches!((source, target), (Some(source), Some(target)) if source > target)
}

/// Java: `PartitionPreprocessor.reverse`.
fn reverse(graph: &mut LGraph, edge: EdgeId) {
    // `reverse_edge` only answers `false` for an edge whose ports are missing. Such an edge was
    // not collected from a node's list of outgoing edges.
    reverse_edge(graph, edge, true);
    // The user's priority adds to ours, so the user can fix an edge that still gets reversed.
    // Java adds `int`s, which wrap around on overflow, and elkjs does the same.
    let edge = &mut graph.edges[edge];
    edge.priority_direction =
        PARTITION_CONSTRAINT_EDGE_PRIORITY.wrapping_add(edge.priority_direction);
}
