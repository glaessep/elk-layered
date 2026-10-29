//! Port of ELK's `LayerConstraintPostprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LayerConstraintPostprocessor.java

use crate::graph::{LGraph, LNodeKind, NodeId};
use crate::intermediate::{IntermediateError, IntermediateResult};
use crate::options::LayerConstraint;

pub fn postprocess_layer_constraints(graph: &mut LGraph) -> IntermediateResult<()> {
    if !graph.layers().is_empty() {
        move_first_and_last_nodes(graph)?;
    }

    if !graph.hidden_nodes.is_empty() {
        restore_hidden_layer_constraint_nodes(graph);
    }

    Ok(())
}

fn move_first_and_last_nodes(graph: &mut LGraph) -> IntermediateResult<()> {
    let first_layer = 0usize;
    let last_layer = graph.layers().len() - 1;
    let mut first_label_nodes = Vec::new();
    let mut last_label_nodes = Vec::new();

    let layered_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();

    for node in layered_nodes {
        match graph.layerless_nodes[node].layer_constraint {
            LayerConstraint::First => {
                ensure_no_incoming_except_label_dummies(graph, node)?;
                graph.set_node_layer(node, first_layer);
                first_label_nodes.extend(adjacent_label_dummies(graph, node, true));
            }
            LayerConstraint::Last => {
                ensure_no_outgoing_except_label_dummies(graph, node)?;
                graph.set_node_layer(node, last_layer);
                last_label_nodes.extend(adjacent_label_dummies(graph, node, false));
            }
            _ => {}
        }
    }

    graph.compact_empty_layers();

    if !first_label_nodes.is_empty() {
        graph.insert_layer(0);
        for node in first_label_nodes {
            graph.set_node_layer(node, 0);
        }
    }

    if !last_label_nodes.is_empty() {
        let layer = graph.layers().len();
        graph.push_layer();
        for node in last_label_nodes {
            graph.set_node_layer(node, layer);
        }
    }

    Ok(())
}

fn adjacent_label_dummies(graph: &LGraph, node: NodeId, incoming: bool) -> Vec<NodeId> {
    let edges = if incoming {
        graph.node_incoming_edges(node)
    } else {
        graph.node_outgoing_edges(node)
    };

    edges
        .into_iter()
        .filter_map(|edge| {
            let candidate = if incoming {
                graph.edges[edge].source().node
            } else {
                graph.edges[edge].target().node
            };
            (graph.layerless_nodes[candidate].kind() == LNodeKind::Label).then_some(candidate)
        })
        .collect()
}

pub(crate) fn restore_hidden_layer_constraint_nodes(graph: &mut LGraph) {
    let hidden_nodes = graph.hidden_nodes.clone();
    let mut first_separate_nodes = Vec::new();
    let mut last_separate_nodes = Vec::new();

    for node in hidden_nodes {
        match graph.layerless_nodes[node].layer_constraint {
            LayerConstraint::FirstSeparate => first_separate_nodes.push(node),
            LayerConstraint::LastSeparate => last_separate_nodes.push(node),
            _ => {}
        }

        restore_hidden_node_edges(graph, node);
    }

    if !first_separate_nodes.is_empty() {
        graph.insert_layer(0);
        for node in first_separate_nodes {
            graph.set_node_layer(node, 0);
        }
    }

    if !last_separate_nodes.is_empty() {
        let layer = graph.layers().len();
        graph.push_layer();
        for node in last_separate_nodes {
            graph.set_node_layer(node, layer);
        }
    }

    graph.hidden_nodes.clear();
}

fn restore_hidden_node_edges(graph: &mut LGraph, node: NodeId) {
    for edge in graph.node_connected_edges(node) {
        if graph.edge_source_attached(edge) && graph.edge_target_attached(edge) {
            continue;
        }

        let Some(original_opposite_port) = graph.edges[edge].original_opposite_port else {
            continue;
        };

        if !graph.edge_target_attached(edge) {
            graph.set_edge_target(edge, original_opposite_port);
        } else if !graph.edge_source_attached(edge) {
            graph.set_edge_source(edge, original_opposite_port);
        }
    }
}

fn ensure_no_incoming_except_label_dummies(graph: &LGraph, node: NodeId) -> IntermediateResult<()> {
    for edge in graph.node_incoming_edges(node) {
        if graph.layerless_nodes[graph.edges[edge].source().node].kind() != LNodeKind::Label {
            return Err(IntermediateError::FirstIncomingEdge {
                node_id: graph.layerless_nodes[node].id.clone(),
                edge_id: graph.edges[edge].id.clone(),
            });
        }
    }
    Ok(())
}

fn ensure_no_outgoing_except_label_dummies(graph: &LGraph, node: NodeId) -> IntermediateResult<()> {
    for edge in graph.node_outgoing_edges(node) {
        if graph.layerless_nodes[graph.edges[edge].target().node].kind() != LNodeKind::Label {
            return Err(IntermediateError::LastOutgoingEdge {
                node_id: graph.layerless_nodes[node].id.clone(),
                edge_id: graph.edges[edge].id.clone(),
            });
        }
    }
    Ok(())
}
