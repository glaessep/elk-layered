//! Port of ELK's `SelfHyperLoopLabels`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/SelfHyperLoopLabels.java

use crate::graph::{LGraph, PortSide, SelfHyperLoopLabels, SelfLoopLabelAlignment};

pub(crate) fn write_self_loop_label_positions(graph: &mut LGraph, labels: &SelfHyperLoopLabels) {
    if graph.options.direction.is_vertical() {
        write_self_loop_label_positions_for_vertical_layout(graph, labels);
    } else {
        write_self_loop_label_positions_for_horizontal_layout(graph, labels);
    }
}

fn write_self_loop_label_positions_for_horizontal_layout(
    graph: &mut LGraph,
    labels: &SelfHyperLoopLabels,
) {
    let mut y = labels.position.y;
    for label_ref in &labels.label_refs {
        let Some(label) = graph
            .edges
            .get_mut(label_ref.edge)
            .and_then(|edge| edge.labels.get_mut(label_ref.label))
        else {
            continue;
        };

        label.position.x = if labels.alignment == Some(SelfLoopLabelAlignment::Left)
            || labels.side == Some(PortSide::East)
        {
            labels.position.x
        } else if labels.alignment == Some(SelfLoopLabelAlignment::Right)
            || labels.side == Some(PortSide::West)
        {
            labels.position.x + labels.size.width - label.size.width
        } else {
            labels.position.x + (labels.size.width - label.size.width) / 2.0
        };
        label.position.y = y;
        y += label.size.height + graph.options.spacing.label_label;
    }
}

fn write_self_loop_label_positions_for_vertical_layout(
    graph: &mut LGraph,
    labels: &SelfHyperLoopLabels,
) {
    let mut x = labels.position.x;
    for label_ref in &labels.label_refs {
        let Some(label) = graph
            .edges
            .get_mut(label_ref.edge)
            .and_then(|edge| edge.labels.get_mut(label_ref.label))
        else {
            continue;
        };

        label.position.x = x;
        label.position.y = if labels.side == Some(PortSide::North) {
            labels.position.y + labels.size.height - label.size.height
        } else {
            labels.position.y
        };
        x += label.size.width + graph.options.spacing.label_label;
    }
}
