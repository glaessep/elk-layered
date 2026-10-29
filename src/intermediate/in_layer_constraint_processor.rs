//! Port of ELK's `InLayerConstraintProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/InLayerConstraintProcessor.java

use crate::graph::{InLayerConstraint, LGraph};

pub fn process_in_layer_constraints(graph: &mut LGraph) {
    for layer_index in 0..graph.layers().len() {
        let nodes = graph.layers()[layer_index].nodes().to_vec();
        let mut top_nodes = Vec::new();
        let mut middle_nodes = Vec::new();
        let mut bottom_nodes = Vec::new();

        for node in nodes {
            match graph.layerless_nodes[node].in_layer_constraint() {
                InLayerConstraint::Top => top_nodes.push(node),
                InLayerConstraint::Bottom => bottom_nodes.push(node),
                InLayerConstraint::None => middle_nodes.push(node),
            }
        }

        top_nodes.extend(middle_nodes);
        top_nodes.extend(bottom_nodes);
        graph.set_layer_nodes(layer_index, top_nodes);
    }
}
