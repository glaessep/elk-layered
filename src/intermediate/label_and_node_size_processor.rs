//! Port of ELK's `LabelAndNodeSizeProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelAndNodeSizeProcessor.java

use crate::common::nodespacing;
use crate::graph::{LGraph, LNodeKind};

pub fn calculate_label_and_node_sizes(graph: &mut LGraph) {
    let layered_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();

    for node in layered_nodes {
        if graph.layerless_nodes[node].kind() == LNodeKind::Normal {
            nodespacing::calculate_label_and_node_sizes(graph, [node]);
        }
    }
}
