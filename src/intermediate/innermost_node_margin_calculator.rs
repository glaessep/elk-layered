//! Port of ELK's `InnermostNodeMarginCalculator`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/InnermostNodeMarginCalculator.java

use crate::common::nodespacing;
use crate::graph::LGraph;

pub fn calculate_innermost_node_margins(graph: &mut LGraph) {
    let layered_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();

    nodespacing::calculate_node_margins(graph, layered_nodes);
}
