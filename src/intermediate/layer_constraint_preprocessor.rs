//! Port of ELK's `LayerConstraintPreprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LayerConstraintPreprocessor.java

use crate::graph::{EdgeId, IdVec, LGraph, LNodeKind, NodeId};
use crate::intermediate::{IntermediateError, IntermediateResult};
use crate::options::LayerConstraint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HiddenNodeConnections {
    None,
    FirstSeparate,
    LastSeparate,
    Both,
}

impl HiddenNodeConnections {
    fn combine(self, layer_constraint: LayerConstraint) -> Self {
        match self {
            Self::None => {
                if layer_constraint == LayerConstraint::FirstSeparate {
                    Self::FirstSeparate
                } else {
                    Self::LastSeparate
                }
            }
            Self::FirstSeparate => {
                if layer_constraint == LayerConstraint::FirstSeparate {
                    Self::FirstSeparate
                } else {
                    Self::Both
                }
            }
            Self::LastSeparate => {
                if layer_constraint == LayerConstraint::FirstSeparate {
                    Self::Both
                } else {
                    Self::LastSeparate
                }
            }
            Self::Both => Self::Both,
        }
    }
}

pub fn preprocess_layer_constraints(graph: &mut LGraph) -> IntermediateResult<()> {
    graph.hidden_nodes.clear();
    let mut hidden_connections =
        IdVec::filled(graph.layerless_nodes.len(), HiddenNodeConnections::None);
    let nodes = graph.node_ids().collect::<Vec<_>>();

    for node in nodes {
        if is_relevant_layer_constraint_node(graph, node) {
            hide_layer_constraint_node(graph, node, &mut hidden_connections)?;
            graph.remove_node_from_layer(node);
            graph.hidden_nodes.push(node);
        }
    }

    Ok(())
}

fn is_relevant_layer_constraint_node(graph: &LGraph, node: NodeId) -> bool {
    matches!(
        graph.layerless_nodes[node].layer_constraint(),
        LayerConstraint::FirstSeparate | LayerConstraint::LastSeparate
    )
}

fn hide_layer_constraint_node(
    graph: &mut LGraph,
    node: NodeId,
    hidden_connections: &mut IdVec<NodeId, HiddenNodeConnections>,
) -> IntermediateResult<()> {
    ensure_no_unacceptable_separate_edges(graph, node)?;
    let connected_edges = graph.node_connected_edges(node);

    for edge in connected_edges {
        hide_layer_constraint_edge(graph, node, edge, hidden_connections);
    }

    Ok(())
}

fn hide_layer_constraint_edge(
    graph: &mut LGraph,
    node: NodeId,
    edge: EdgeId,
    hidden_connections: &mut IdVec<NodeId, HiddenNodeConnections>,
) {
    let is_outgoing = graph.edges[edge].source().node == node;
    let opposite_port = if is_outgoing {
        graph.edges[edge].target()
    } else {
        graph.edges[edge].source()
    };

    if is_outgoing {
        graph.detach_edge_target(edge);
    } else {
        graph.detach_edge_source(edge);
    }
    graph.edges[edge].original_opposite_port = Some(opposite_port);

    update_opposite_node_layer_constraints(graph, node, opposite_port.node, hidden_connections);
}

fn update_opposite_node_layer_constraints(
    graph: &mut LGraph,
    hidden_node: NodeId,
    opposite_node: NodeId,
    hidden_connections: &mut IdVec<NodeId, HiddenNodeConnections>,
) {
    if graph.layerless_nodes[opposite_node].is_layer_constraint_explicit() {
        return;
    }

    hidden_connections[opposite_node] = hidden_connections[opposite_node]
        .combine(graph.layerless_nodes[hidden_node].layer_constraint());

    if !graph.node_connected_edges(opposite_node).is_empty() {
        return;
    }

    match hidden_connections[opposite_node] {
        HiddenNodeConnections::FirstSeparate => {
            graph.layerless_nodes[opposite_node].set_layer_constraint(LayerConstraint::First);
        }
        HiddenNodeConnections::LastSeparate => {
            graph.layerless_nodes[opposite_node].set_layer_constraint(LayerConstraint::Last);
        }
        HiddenNodeConnections::None | HiddenNodeConnections::Both => {}
    }
}

fn ensure_no_unacceptable_separate_edges(graph: &LGraph, node: NodeId) -> IntermediateResult<()> {
    match graph.layerless_nodes[node].layer_constraint() {
        LayerConstraint::FirstSeparate => {
            for edge in graph.node_incoming_edges(node) {
                if !is_acceptable_separate_incident_edge(graph, edge) {
                    return Err(IntermediateError::FirstSeparateIncomingEdge {
                        node_id: graph.layerless_nodes[node].id.clone(),
                        edge_id: graph.edges[edge].id.clone(),
                    });
                }
            }
        }
        LayerConstraint::LastSeparate => {
            for edge in graph.node_outgoing_edges(node) {
                if !is_acceptable_separate_incident_edge(graph, edge) {
                    return Err(IntermediateError::LastSeparateOutgoingEdge {
                        node_id: graph.layerless_nodes[node].id.clone(),
                        edge_id: graph.edges[edge].id.clone(),
                    });
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn is_acceptable_separate_incident_edge(graph: &LGraph, edge: EdgeId) -> bool {
    graph.layerless_nodes[graph.edges[edge].source().node].kind() == LNodeKind::ExternalPort
        && graph.layerless_nodes[graph.edges[edge].target().node].kind() == LNodeKind::ExternalPort
}
