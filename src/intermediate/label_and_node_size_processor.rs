//! Port of ELK's `LabelAndNodeSizeProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelAndNodeSizeProcessor.java

use crate::common::nodespacing;
use crate::graph::{LGraph, LNodeKind, LPoint, NodeId, OtherGraphs, PortId, PortSide};
use crate::options::PortLabelPlacement;

/// Sizes the nodes of the graph and places their ports and labels, then places the labels of the
/// external port dummies.
///
/// Ports `LabelAndNodeSizeProcessor.process`. `others` are the graphs inside the nodes of
/// `graph`: the edges at the dummy of a port, inside its node, count as edges of the port.
pub fn calculate_label_and_node_sizes(graph: &mut LGraph, others: OtherGraphs<'_>) {
    let layered_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();

    for &node in &layered_nodes {
        if graph.layerless_nodes[node].kind() == LNodeKind::Normal {
            nodespacing::calculate_label_and_node_sizes(graph, others, [node]);
        }
    }

    // If the graph has external ports, ELK treats the labels of the external port dummies a bit
    // differently, which is why it did not hand them to the code above.
    if graph.graph_properties.external_ports {
        let placement = graph.options.port_label_placement;
        let place_next_to_port = placement.contains(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);
        let treat_as_group = graph.options.port_labels_treat_as_group;
        for &node in &layered_nodes {
            if graph.layerless_nodes[node].kind() == LNodeKind::ExternalPort {
                place_external_port_dummy_labels(
                    graph,
                    node,
                    placement,
                    place_next_to_port,
                    treat_as_group,
                );
            }
        }
    }
}

/// Places the labels of an external port dummy so the node margins reserve room for them once the
/// labels are placed for good.
///
/// Ports `LabelAndNodeSizeProcessor.placeExternalPortDummyLabels`.
pub(crate) fn place_external_port_dummy_labels(
    graph: &mut LGraph,
    dummy: NodeId,
    graph_placement: PortLabelPlacement,
    place_next_to_port_if_possible: bool,
    treat_as_group: bool,
) {
    let data = &graph.layerless_nodes[dummy];
    let Some(external) = data.external_port() else {
        return;
    };
    let spacings = external.label_spacings;
    let side = external.side;
    let dummy_size = data.size;
    // External port dummies have exactly one port.
    let dummy_port = PortId::new(0);
    let port_position = data.ports[dummy_port].position;
    let Some(box_size) = port_label_box(graph, dummy, dummy_port, spacings.label_label) else {
        return;
    };
    let (box_width, box_height) = box_size;
    let (mut box_x, mut box_y) = (0.0, 0.0);
    let first_label_height = graph.layerless_nodes[dummy].ports[dummy_port].labels[0]
        .size
        .height;
    let label_height = if treat_as_group {
        box_height
    } else {
        first_label_height
    };

    if graph_placement.contains(PortLabelPlacement::INSIDE) {
        // Port label placement has to support this case first, though.
        match side {
            PortSide::North => {
                box_x = (dummy_size.width - box_width) / 2.0 - port_position.x;
                box_y = spacings.label_port_vertical;
            }
            PortSide::South => {
                box_x = (dummy_size.width - box_width) / 2.0 - port_position.x;
                box_y = -spacings.label_port_vertical - box_height;
            }
            PortSide::East | PortSide::West => {
                if label_next_to_port(
                    graph,
                    dummy,
                    dummy_port,
                    true,
                    place_next_to_port_if_possible,
                ) {
                    box_y = (dummy_size.height - label_height) / 2.0 - port_position.y;
                } else {
                    box_y = dummy_size.height + spacings.label_port_vertical - port_position.y;
                }
                box_x = if side == PortSide::East {
                    -spacings.label_port_horizontal - box_width
                } else {
                    spacings.label_port_horizontal
                };
            }
            PortSide::Undefined => {}
        }
    } else if graph_placement.contains(PortLabelPlacement::OUTSIDE) {
        match side {
            PortSide::North | PortSide::South => {
                box_x = port_position.x + spacings.label_port_horizontal;
            }
            PortSide::East | PortSide::West => {
                if label_next_to_port(
                    graph,
                    dummy,
                    dummy_port,
                    false,
                    place_next_to_port_if_possible,
                ) {
                    box_y = (dummy_size.height - label_height) / 2.0 - port_position.y;
                } else {
                    box_y = port_position.y + spacings.label_port_vertical;
                }
            }
            PortSide::Undefined => {}
        }
    }

    // Place the labels.
    let mut current_y = box_y;
    for label in &mut graph.layerless_nodes[dummy].ports[dummy_port].labels {
        label.position = LPoint {
            x: box_x,
            y: current_y,
        };
        current_y += label.size.height + spacings.label_label;
    }
}

/// The size of the box that holds the labels of the dummy's port, or `None` without labels.
///
/// Ports `LabelAndNodeSizeProcessor.computePortLabelBox`, which gives the box its size alone.
fn port_label_box(
    graph: &LGraph,
    dummy: NodeId,
    dummy_port: PortId,
    label_label_spacing: f64,
) -> Option<(f64, f64)> {
    let labels = &graph.layerless_nodes[dummy].ports[dummy_port].labels;
    if labels.is_empty() {
        return None;
    }
    let mut width: f64 = 0.0;
    let mut height = 0.0;
    for label in labels {
        width = width.max(label.size.width);
        height += label.size.height;
    }
    height += (labels.len() - 1) as f64 * label_label_spacing;
    Some((width, height))
}

/// Whether the labels of the dummy's port go next to the port rather than below it: the caller
/// asked for that if possible, and the port has no connection.
///
/// Ports `LabelAndNodeSizeProcessor.labelNextToPort`.
fn label_next_to_port(
    graph: &LGraph,
    dummy: NodeId,
    dummy_port: PortId,
    inside_labels: bool,
    place_next_to_port_if_possible: bool,
) -> bool {
    if !place_next_to_port_if_possible {
        return false;
    }
    let port = &graph.layerless_nodes[dummy].ports[dummy_port];
    if inside_labels {
        port.incoming_edges().is_empty() && port.outgoing_edges().is_empty()
    } else {
        !port.connected_to_external_nodes
    }
}
