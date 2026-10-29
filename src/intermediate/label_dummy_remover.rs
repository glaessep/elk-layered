//! Port of ELK's `LabelDummyRemover`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelDummyRemover.java

use crate::graph::{LGraph, LLabel, LNodeKind, LPoint, LabelSide, NodeId};
use crate::intermediate::long_edge_joiner::join_long_edge_at;
use crate::options::ElkDirection;

pub fn remove_label_dummies(graph: &mut LGraph) {
    for layer_index in 0..graph.layers().len() {
        let mut node_position = 0usize;

        while node_position < graph.layers()[layer_index].nodes().len() {
            let node = graph.layers()[layer_index].nodes()[node_position];
            if graph.layerless_nodes[node].kind() == LNodeKind::Label {
                place_label_dummy_labels(graph, node);
                let add_unnecessary_bendpoints =
                    graph.options.edge_routing == crate::options::EdgeRouting::Polyline;
                join_long_edge_at(graph, node, add_unnecessary_bendpoints);
                graph.remove_node_at(layer_index, node_position);
            } else {
                node_position += 1;
            }
        }
    }
}

pub(crate) fn place_label_dummy_labels(graph: &mut LGraph, label_dummy: NodeId) {
    let Some(label_data) = graph.layerless_nodes[label_dummy].label_dummy() else {
        return;
    };
    let Some(origin_edge) = label_data.origin_edge else {
        return;
    };
    let thickness = graph.edges[origin_edge].thickness.max(0.0);
    let edge_label_spacing = graph.options.spacing.edge_label;
    let label_label_spacing = graph.options.spacing.label_label;
    let labels_below_edge = label_data.side == LabelSide::Below;
    let inline = graph.layerless_nodes[label_dummy]
        .represented_labels()
        .iter()
        .all(|label| label.inline);
    let mut label_position = graph.layerless_nodes[label_dummy].position;

    if labels_below_edge {
        label_position.y += thickness + edge_label_spacing;
    }

    let label_space = LPoint {
        x: graph.layerless_nodes[label_dummy].size.width,
        y: graph.layerless_nodes[label_dummy].size.height
            + if inline {
                0.0
            } else {
                -thickness - edge_label_spacing
            },
    };
    let mut labels = graph.layerless_nodes[label_dummy]
        .label_dummy_mut()
        .map(|label_data| std::mem::take(&mut label_data.represented_labels))
        .unwrap_or_default();

    if graph.options.direction.is_vertical() {
        place_labels_for_vertical_layout(
            &mut labels,
            label_position,
            label_label_spacing,
            label_space,
            labels_below_edge,
            graph.options.direction,
        );
    } else {
        place_labels_for_horizontal_layout(
            &mut labels,
            label_position,
            label_label_spacing,
            label_space,
        );
    }

    graph.edges[origin_edge].labels.extend(labels);
}

fn place_labels_for_horizontal_layout(
    labels: &mut [LLabel],
    mut label_position: LPoint,
    label_spacing: f64,
    label_space: LPoint,
) {
    for label in labels {
        label.position.x = label_position.x + (label_space.x - label.size.width) / 2.0;
        label.position.y = label_position.y;
        label_position.y += label.size.height + label_spacing;
    }
}

fn place_labels_for_vertical_layout(
    labels: &mut [LLabel],
    mut label_position: LPoint,
    label_spacing: f64,
    label_space: LPoint,
    left_aligned: bool,
    direction: ElkDirection,
) {
    let inline = labels.iter().all(|label| label.inline);

    if direction == ElkDirection::Up {
        labels.reverse();
    }

    for label in &mut *labels {
        label.position.x = label_position.x;
        if inline {
            label.position.y = label_position.y + (label_space.y - label.size.height) / 2.0;
        } else if left_aligned {
            label.position.y = label_position.y;
        } else {
            label.position.y = label_position.y + label_space.y - label.size.height;
        }
        label_position.x += label.size.width + label_spacing;
    }

    if direction == ElkDirection::Up {
        labels.reverse();
    }
}
