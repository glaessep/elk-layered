//! Port of ELK's `LabelSideSelector`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LabelSideSelector.java

use std::collections::VecDeque;

use crate::graph::{EdgeId, LGraph, LNodeKind, LabelSide, NodeId, PortSide};
use crate::intermediate::end_label_preprocessor::{EndLabelRef, gather_end_labels};
use crate::options::EdgeLabelSideSelection;

pub fn select_label_sides(graph: &mut LGraph) {
    match graph.options.edge_label_side_selection {
        EdgeLabelSideSelection::AlwaysUp => same_side(graph, LabelSide::Above),
        EdgeLabelSideSelection::AlwaysDown => same_side(graph, LabelSide::Below),
        EdgeLabelSideSelection::DirectionUp => based_on_direction(graph, LabelSide::Above),
        EdgeLabelSideSelection::DirectionDown => based_on_direction(graph, LabelSide::Below),
        EdgeLabelSideSelection::SmartUp => smart(graph, LabelSide::Above),
        EdgeLabelSideSelection::SmartDown => smart(graph, LabelSide::Below),
    }
}

fn same_side(graph: &mut LGraph, label_side: LabelSide) {
    for layer_index in 0..graph.layers().len() {
        let nodes = graph.layers()[layer_index].nodes().to_vec();
        for node in nodes {
            if graph.layerless_nodes[node].kind() == LNodeKind::Label {
                apply_label_side(graph, node, label_side);
            }

            let outgoing_edges = graph.node_outgoing_edges(node);
            for edge in outgoing_edges {
                apply_label_side_to_edge(graph, edge, label_side);
            }
        }
    }
}

fn based_on_direction(graph: &mut LGraph, side_for_rightward_edges: LabelSide) {
    for layer_index in 0..graph.layers().len() {
        let nodes = graph.layers()[layer_index].nodes().to_vec();
        for node in nodes {
            if graph.layerless_nodes[node].kind() == LNodeKind::Label {
                let side = if does_label_dummy_point_right(graph, node) {
                    side_for_rightward_edges
                } else {
                    side_for_rightward_edges.opposite()
                };
                apply_label_side(graph, node, side);
            }

            let outgoing_edges = graph.node_outgoing_edges(node);
            for edge in outgoing_edges {
                let side = if !graph.edges[edge].reversed {
                    side_for_rightward_edges
                } else {
                    side_for_rightward_edges.opposite()
                };
                apply_label_side_to_edge(graph, edge, side);
            }
        }
    }
}

fn smart(graph: &mut LGraph, default_side: LabelSide) {
    let mut dummy_node_queue = VecDeque::new();

    for layer_index in 0..graph.layers().len() {
        let nodes = graph.layers()[layer_index].nodes().to_vec();
        let mut top_group = true;
        let mut label_dummies_in_queue = 0usize;

        for node in nodes {
            match graph.layerless_nodes[node].kind() {
                LNodeKind::Label => {
                    label_dummies_in_queue += 1;
                    dummy_node_queue.push_back(node);
                }
                LNodeKind::LongEdge => {
                    dummy_node_queue.push_back(node);
                }
                LNodeKind::Normal => {
                    smart_for_regular_node(graph, node, default_side);
                    if !dummy_node_queue.is_empty() {
                        smart_for_consecutive_dummy_node_run(
                            graph,
                            &mut dummy_node_queue,
                            label_dummies_in_queue,
                            top_group,
                            false,
                            default_side,
                        );
                    }
                    top_group = false;
                    label_dummies_in_queue = 0;
                }
                _ => {
                    if !dummy_node_queue.is_empty() {
                        smart_for_consecutive_dummy_node_run(
                            graph,
                            &mut dummy_node_queue,
                            label_dummies_in_queue,
                            top_group,
                            false,
                            default_side,
                        );
                    }
                    top_group = false;
                    label_dummies_in_queue = 0;
                }
            }
        }

        if !dummy_node_queue.is_empty() {
            smart_for_consecutive_dummy_node_run(
                graph,
                &mut dummy_node_queue,
                label_dummies_in_queue,
                top_group,
                true,
                default_side,
            );
        }
    }
}

fn smart_for_consecutive_dummy_node_run(
    graph: &mut LGraph,
    dummy_nodes: &mut VecDeque<NodeId>,
    label_dummy_count: usize,
    top_group: bool,
    bottom_group: bool,
    default_side: LabelSide,
) {
    debug_assert!(!dummy_nodes.is_empty());

    if top_group
        && (!bottom_group || dummy_nodes.len() > 1)
        && label_dummy_count == 1
        && dummy_nodes
            .front()
            .is_some_and(|node| graph.layerless_nodes[*node].kind() == LNodeKind::Label)
    {
        if let Some(node) = dummy_nodes.front().copied() {
            apply_label_side(graph, node, LabelSide::Above);
        }
    } else if bottom_group
        && (!top_group || dummy_nodes.len() > 1)
        && label_dummy_count == 1
        && dummy_nodes
            .back()
            .is_some_and(|node| graph.layerless_nodes[*node].kind() == LNodeKind::Label)
    {
        if let Some(node) = dummy_nodes.back().copied() {
            apply_label_side(graph, node, LabelSide::Below);
        }
    } else if dummy_nodes.len() == 2 {
        if let Some(node) = dummy_nodes.pop_front() {
            apply_label_side(graph, node, LabelSide::Above);
        }
        if let Some(node) = dummy_nodes.pop_front() {
            apply_label_side(graph, node, LabelSide::Below);
        }
    } else {
        apply_for_dummy_node_run_with_simple_loops(graph, dummy_nodes, default_side);
    }

    dummy_nodes.clear();
}

fn apply_for_dummy_node_run_with_simple_loops(
    graph: &mut LGraph,
    dummy_nodes: &VecDeque<NodeId>,
    default_side: LabelSide,
) {
    let mut label_dummy_run = Vec::with_capacity(dummy_nodes.len());
    let mut prev_long_edge_source = None;
    let mut prev_long_edge_target = None;

    for current_dummy in dummy_nodes.iter().copied() {
        let curr_long_edge_source = get_long_edge_end_node(graph, current_dummy, true);
        let curr_long_edge_target = get_long_edge_end_node(graph, current_dummy, false);

        if prev_long_edge_source != curr_long_edge_source
            || prev_long_edge_target != curr_long_edge_target
        {
            apply_label_sides_to_label_dummy_run(graph, &mut label_dummy_run, default_side);
            prev_long_edge_source = curr_long_edge_source;
            prev_long_edge_target = curr_long_edge_target;
        }

        label_dummy_run.push(current_dummy);
    }

    apply_label_sides_to_label_dummy_run(graph, &mut label_dummy_run, default_side);
}

fn get_long_edge_end_node(graph: &LGraph, label_dummy: NodeId, source: bool) -> Option<NodeId> {
    let ends = graph.layerless_nodes[label_dummy].long_edge_ends()?;
    let port = if source { ends.source } else { ends.target }?;
    Some(port.node)
}

fn apply_label_sides_to_label_dummy_run(
    graph: &mut LGraph,
    label_dummy_run: &mut Vec<NodeId>,
    default_side: LabelSide,
) {
    if label_dummy_run.is_empty() {
        return;
    }

    if label_dummy_run.len() == 2 {
        if let Some(node) = label_dummy_run.first().copied() {
            apply_label_side(graph, node, LabelSide::Above);
        }
        if let Some(node) = label_dummy_run.get(1).copied() {
            apply_label_side(graph, node, LabelSide::Below);
        }
    } else {
        for dummy_node in label_dummy_run.iter().copied() {
            apply_label_side(graph, dummy_node, default_side);
        }
    }

    label_dummy_run.clear();
}

pub(crate) fn smart_for_regular_node(graph: &mut LGraph, node: NodeId, default_side: LabelSide) {
    let mut end_label_queue: VecDeque<Vec<EndLabelRef>> = VecDeque::new();
    let mut current_port_side = None;

    // Gathering end labels reads edges and writes labels; it never changes the node's list.
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port_index) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        let port_side = graph.layerless_nodes[node].ports[port_index].side;
        if Some(port_side) != current_port_side {
            if !end_label_queue.is_empty() {
                smart_for_regular_node_port_end_labels(
                    graph,
                    &mut end_label_queue,
                    current_port_side,
                    default_side,
                );
            }

            end_label_queue.clear();
            current_port_side = Some(port_side);
        }

        // A port with an edge counts, even when no end label sits on that edge.
        if let Some(gathered) = gather_end_labels(graph, node, port_index) {
            end_label_queue.push_back(gathered.labels);
        }
    }

    if !end_label_queue.is_empty() {
        smart_for_regular_node_port_end_labels(
            graph,
            &mut end_label_queue,
            current_port_side,
            default_side,
        );
    }
}

fn smart_for_regular_node_port_end_labels(
    graph: &mut LGraph,
    end_label_queue: &mut VecDeque<Vec<EndLabelRef>>,
    port_side: Option<PortSide>,
    default_side: LabelSide,
) {
    debug_assert!(!end_label_queue.is_empty());
    let Some(port_side) = port_side else {
        return;
    };

    if end_label_queue.len() == 2 {
        let (first, second) = if matches!(port_side, PortSide::North | PortSide::East) {
            (LabelSide::Above, LabelSide::Below)
        } else {
            (LabelSide::Below, LabelSide::Above)
        };
        if let Some(labels) = end_label_queue.pop_front() {
            apply_label_side_to_end_labels(graph, &labels, first);
        }
        if let Some(labels) = end_label_queue.pop_front() {
            apply_label_side_to_end_labels(graph, &labels, second);
        }
    } else {
        for labels in end_label_queue.iter() {
            apply_label_side_to_end_labels(graph, labels, default_side);
        }
    }

    end_label_queue.clear();
}

fn apply_label_side_to_end_labels(graph: &mut LGraph, labels: &[EndLabelRef], side: LabelSide) {
    for label in labels {
        graph.edges[label.edge].labels[label.index].label_side = Some(side);
    }
}

fn apply_label_side_to_edge(graph: &mut LGraph, edge: EdgeId, side: LabelSide) {
    for label in &mut graph.edges[edge].labels {
        label.label_side = Some(side);
    }
}

pub(crate) fn apply_label_side(graph: &mut LGraph, label_dummy: NodeId, side: LabelSide) {
    let Some(label_data) = graph.layerless_nodes[label_dummy].label_dummy_mut() else {
        return;
    };

    let effective_side = if label_data
        .represented_labels
        .iter()
        .all(|label| label.inline)
    {
        LabelSide::Inline
    } else {
        side
    };

    label_data.side = effective_side;
    for label in &mut label_data.represented_labels {
        label.label_side = Some(effective_side);
    }

    if effective_side == LabelSide::Below {
        return;
    }

    let origin_edge = graph.layerless_nodes[label_dummy]
        .origin_edge()
        .and_then(|edge| graph.edges.get(edge))
        .map(|edge| edge.thickness.max(0.0))
        .unwrap_or(0.0);
    let port_y = match effective_side {
        LabelSide::Above => {
            graph.layerless_nodes[label_dummy].size.height - (origin_edge / 2.0).ceil()
        }
        LabelSide::Inline => {
            let edge_label_spacing = graph.options.spacing.edge_label;
            let port_y =
                (graph.layerless_nodes[label_dummy].size.height - edge_label_spacing - origin_edge)
                    .ceil()
                    / 2.0;
            graph.layerless_nodes[label_dummy].size.height -= edge_label_spacing;
            graph.layerless_nodes[label_dummy].size.height -= origin_edge;
            port_y
        }
        LabelSide::Below => 0.0,
    };

    for port in graph.layerless_nodes[label_dummy].ports.slots_mut() {
        port.position.y = port_y;
    }
}

fn does_label_dummy_point_right(graph: &LGraph, label_dummy: NodeId) -> bool {
    let incoming = graph.node_incoming_edges(label_dummy).into_iter().next();
    let outgoing = graph.node_outgoing_edges(label_dummy).into_iter().next();

    incoming
        .map(|edge| !graph.edges[edge].reversed)
        .unwrap_or(false)
        || outgoing
            .map(|edge| !graph.edges[edge].reversed)
            .unwrap_or(false)
}
