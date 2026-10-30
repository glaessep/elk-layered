//! Port of ELK's `EndLabelPostprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/EndLabelPostprocessor.java

use crate::graph::{
    HorizontalLabelAlignment, LGraph, LNodeKind, LPoint, LabelCellLayout, NodeId, PortId,
    VerticalLabelAlignment,
};

pub fn postprocess_end_labels(graph: &mut LGraph) {
    for node in graph.node_ids() {
        if !matches!(
            graph.layerless_nodes[node].kind(),
            LNodeKind::Normal | LNodeKind::ExternalPort
        ) {
            continue;
        }
        let node_position = graph.layerless_nodes[node].position;
        // Applying a cell layout writes labels and positions; it never changes the node's list.
        for position in 0..graph.layerless_nodes[node].ports.len() {
            let Some(port_index) = graph.layerless_nodes[node].ports.id_at(position) else {
                continue;
            };
            let Some(cell) = graph.layerless_nodes[node].ports[port_index].end_label_cell else {
                continue;
            };
            // Java walks the layers, so a node that left its layer keeps its end labels where
            // they are. This crate holds them at the port, so it hands them back unplaced.
            if graph.layerless_nodes[node].layer_index().is_none() {
                hand_back_end_labels(graph, node, port_index);
                continue;
            }
            apply_end_label_cell_layout(graph, node, port_index, node_position, cell);
            graph.layerless_nodes[node].ports[port_index].end_label_cell = None;
        }
    }
}

fn apply_end_label_cell_layout(
    graph: &mut LGraph,
    node: NodeId,
    port_index: PortId,
    node_position: LPoint,
    cell: LabelCellLayout,
) {
    let label_label_spacing = graph.options.spacing.label_label;
    let mut labels = std::mem::take(&mut graph.layerless_nodes[node].ports[port_index].end_labels);
    let mut cursor = cell.position;
    cursor.x += node_position.x;
    cursor.y += node_position.y;

    // `LabelCell.applyHorizontalModeLabelLayout` stacks the labels one below the other, and
    // `applyVerticalModeLabelLayout` puts them side by side. The cell is exactly as large as its
    // labels, so the alignment offset of the first label is zero.
    for label in &mut labels {
        if cell.horizontal_layout {
            label.position.x = aligned_x(cursor.x, cell.size.width, label.size.width, cell);
            label.position.y = cursor.y;
            cursor.y += label.size.height + label_label_spacing;
        } else {
            label.position.x = cursor.x;
            label.position.y = aligned_y(cursor.y, cell.size.height, label.size.height, cell);
            cursor.x += label.size.width + label_label_spacing;
        }
    }

    return_labels_to_edges(graph, node, port_index, labels);
}

/// Puts end labels back on the edges they belong to.
/// Hands the end labels that the port `port_index` of `node` holds back to their edges, unplaced,
/// as a node that `EndLabelPostprocessor` never visits keeps them in Java.
pub(crate) fn hand_back_end_labels(graph: &mut LGraph, node: NodeId, port_index: PortId) {
    let labels = std::mem::take(&mut graph.layerless_nodes[node].ports[port_index].end_labels);
    return_labels_to_edges(graph, node, port_index, labels);
    graph.layerless_nodes[node].ports[port_index].end_label_cell = None;
}

fn return_labels_to_edges(
    graph: &mut LGraph,
    node: NodeId,
    port_index: PortId,
    labels: Vec<crate::graph::LLabel>,
) {
    for label in labels {
        let target_edge = label.end_label_edge.or_else(|| {
            let port = &graph.layerless_nodes[node].ports[port_index];
            port.incoming_edges()
                .first()
                .or_else(|| port.outgoing_edges().first())
                .copied()
        });
        if let Some(edge) = target_edge.and_then(|id| graph.edges.get_mut(id)) {
            edge.labels.push(label);
        }
    }
}

fn aligned_x(x: f64, cell_width: f64, label_width: f64, cell: LabelCellLayout) -> f64 {
    match cell.horizontal_alignment {
        HorizontalLabelAlignment::Left => x,
        HorizontalLabelAlignment::Center => x + (cell_width - label_width) / 2.0,
        HorizontalLabelAlignment::Right => x + cell_width - label_width,
    }
}

fn aligned_y(y: f64, cell_height: f64, label_height: f64, cell: LabelCellLayout) -> f64 {
    match cell.vertical_alignment {
        VerticalLabelAlignment::Top => y,
        VerticalLabelAlignment::Center => y + (cell_height - label_height) / 2.0,
        VerticalLabelAlignment::Bottom => y + cell_height - label_height,
    }
}
