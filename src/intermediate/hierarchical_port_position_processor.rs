//! Port of ELK's `HierarchicalPortPositionProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalPortPositionProcessor.java

use crate::graph::{LGraph, LPoint, NodeId, PortSide};

pub fn process_hierarchical_port_positions(graph: &mut LGraph) {
    if !(graph.options.port_constraints.is_ratio_fixed()
        || graph.options.port_constraints.is_pos_fixed())
        || graph.layers().is_empty()
    {
        return;
    }

    process_hierarchical_port_positions_for_layer(graph, 0);
    if graph.layers().len() > 1 {
        process_hierarchical_port_positions_for_layer(graph, graph.layers().len() - 1);
    }
}

fn process_hierarchical_port_positions_for_layer(graph: &mut LGraph, layer_index: usize) {
    let graph_height = actual_graph_height(graph);
    let nodes = graph.layers()[layer_index].nodes().to_vec();

    for node in nodes {
        let Some(external) = graph.layerless_nodes[node].external_port() else {
            continue;
        };
        if !matches!(external.side, PortSide::East | PortSide::West) {
            continue;
        }

        let mut final_y_coordinate = external.ratio_or_position;
        if graph.options.port_constraints.is_ratio_fixed() {
            final_y_coordinate *= graph_height;
        }

        let port_anchor_y = external_port_anchor(graph, node).y;
        graph.layerless_nodes[node].position.y = final_y_coordinate - port_anchor_y;
        border_to_content_area_y(graph, node);
    }
}

pub(crate) fn actual_graph_height(graph: &LGraph) -> f64 {
    graph.size.height + graph.padding.top + graph.padding.bottom
}

/// The `LayeredOptions.PORT_ANCHOR` of an external port dummy, which
/// `HierarchicalPortPositionProcessor.fixCoordinates` and
/// `HierarchicalPortOrthogonalEdgeRouter.fixCoordinates`, `calculateNorthSouthDummyPositions` and
/// `applyNorthSouthDummyRatio` read. It is not the position of the dummy's port: Java turns the
/// port with the graph, but not this property.
pub(crate) fn external_port_anchor(graph: &LGraph, node: NodeId) -> LPoint {
    graph.layerless_nodes[node]
        .external_port()
        .map_or(LPoint::default(), |external| external.anchor)
}

pub(crate) fn border_to_content_area_y(graph: &mut LGraph, node: NodeId) {
    graph.layerless_nodes[node].position.y -= graph.padding.top + graph.offset.y;
}
