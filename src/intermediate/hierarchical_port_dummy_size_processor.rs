//! Port of ELK's `HierarchicalPortDummySizeProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalPortDummySizeProcessor.java

use crate::graph::{LGraph, NodeId, PortSide};
use crate::options::Alignment;

pub fn process_hierarchical_port_dummy_sizes(graph: &mut LGraph) {
    let delta = graph.options.spacing.edge_edge_between_layers * 2.0;

    for layer_index in 0..graph.layers().len() {
        let (northern_dummies, southern_dummies) =
            collect_north_south_external_port_dummies(graph, layer_index);
        set_hierarchical_port_dummy_widths(graph, &northern_dummies, true, delta);
        set_hierarchical_port_dummy_widths(graph, &southern_dummies, false, delta);
    }
}

fn collect_north_south_external_port_dummies(
    graph: &LGraph,
    layer_index: usize,
) -> (Vec<NodeId>, Vec<NodeId>) {
    let mut northern_dummies = Vec::new();
    let mut southern_dummies = Vec::new();

    for node_index in graph.layers()[layer_index].nodes() {
        let Some(external) = graph.layerless_nodes[*node_index].external_port() else {
            continue;
        };

        match external.side {
            PortSide::North => northern_dummies.push(*node_index),
            PortSide::South => southern_dummies.push(*node_index),
            PortSide::Undefined | PortSide::East | PortSide::West => {}
        }
    }

    (northern_dummies, southern_dummies)
}

fn set_hierarchical_port_dummy_widths(
    graph: &mut LGraph,
    nodes: &[NodeId],
    top_down: bool,
    delta: f64,
) {
    let mut current_width = if top_down {
        0.0
    } else {
        delta * nodes.len().saturating_sub(1) as f64
    };
    let step = if top_down { delta } else { -delta };

    for node_index in nodes {
        let node = &mut graph.layerless_nodes[*node_index];
        node.node_alignment = Alignment::Center;
        node.size.width = current_width;

        for port in node.ports.slots_mut() {
            if port.side == PortSide::East {
                port.position.x = current_width;
            }
        }

        current_width += step;
    }
}
