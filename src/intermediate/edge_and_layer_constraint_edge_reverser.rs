//! Port of ELK's `EdgeAndLayerConstraintEdgeReverser`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/EdgeAndLayerConstraintEdgeReverser.java

use crate::graph::{EdgeId, LGraph, LNodeKind, NodeId, PortSide, reverse_edge};
use crate::options::LayerConstraint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeReversalPortType {
    Input,
    Output,
    All,
}

pub fn reverse_edges_for_edge_and_layer_constraints(graph: &mut LGraph) {
    let mut remaining_nodes = Vec::new();

    for node in graph.node_ids() {
        let layer_constraint = graph.layerless_nodes[node].layer_constraint;
        if let Some(target_port_type) = target_port_type_for_layer_constraint(layer_constraint) {
            reverse_node_edges(graph, node, layer_constraint, target_port_type);
        } else {
            remaining_nodes.push(node);
        }
    }

    for node in remaining_nodes {
        if should_reverse_all_fixed_side_edges(graph, node) {
            let layer_constraint = graph.layerless_nodes[node].layer_constraint;
            reverse_node_edges(graph, node, layer_constraint, EdgeReversalPortType::All);
        }
    }
}

fn target_port_type_for_layer_constraint(
    layer_constraint: LayerConstraint,
) -> Option<EdgeReversalPortType> {
    match layer_constraint {
        LayerConstraint::First | LayerConstraint::FirstSeparate => {
            Some(EdgeReversalPortType::Output)
        }
        LayerConstraint::Last | LayerConstraint::LastSeparate => Some(EdgeReversalPortType::Input),
        LayerConstraint::None => None,
    }
}

fn should_reverse_all_fixed_side_edges(graph: &LGraph, node: NodeId) -> bool {
    let lnode = &graph.layerless_nodes[node];
    if !lnode.port_constraints.is_side_fixed() || lnode.ports.is_empty() {
        return false;
    }

    for port in &lnode.ports {
        let reversed_port = match port.side {
            PortSide::East => port.net_flow() > 0,
            PortSide::West => port.net_flow() < 0,
            PortSide::North | PortSide::South | PortSide::Undefined => false,
        };
        if !reversed_port {
            return false;
        }

        for edge in port.outgoing_edges() {
            let target_layer_constraint =
                graph.layerless_nodes[graph.edges[*edge].target().node].layer_constraint;
            if matches!(
                target_layer_constraint,
                LayerConstraint::Last | LayerConstraint::LastSeparate
            ) {
                return false;
            }
        }
        for edge in port.incoming_edges() {
            let source_layer_constraint =
                graph.layerless_nodes[graph.edges[*edge].source().node].layer_constraint;
            if matches!(
                source_layer_constraint,
                LayerConstraint::First | LayerConstraint::FirstSeparate
            ) {
                return false;
            }
        }
    }

    true
}

fn reverse_node_edges(
    graph: &mut LGraph,
    node: NodeId,
    node_layer_constraint: LayerConstraint,
    target_port_type: EdgeReversalPortType,
) {
    // `reverse_edge` with `adapt_ports` can add a collector port to the node's list, so the loop
    // walks a copy of the ids.
    let port_ids = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
    for port in port_ids {
        if matches!(
            target_port_type,
            EdgeReversalPortType::Input | EdgeReversalPortType::All
        ) {
            let outgoing_edges = graph.layerless_nodes[node].ports[port]
                .outgoing_edges()
                .to_vec();
            for edge in outgoing_edges {
                if can_reverse_outgoing_edge(graph, node_layer_constraint, edge) {
                    reverse_edge(graph, edge, true);
                }
            }
        }

        if matches!(
            target_port_type,
            EdgeReversalPortType::Output | EdgeReversalPortType::All
        ) {
            let incoming_edges = graph.layerless_nodes[node].ports[port]
                .incoming_edges()
                .to_vec();
            for edge in incoming_edges {
                if can_reverse_incoming_edge(graph, node_layer_constraint, edge) {
                    reverse_edge(graph, edge, true);
                }
            }
        }
    }
}

fn can_reverse_outgoing_edge(
    graph: &LGraph,
    source_node_layer_constraint: LayerConstraint,
    edge: EdgeId,
) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    if edge.reversed {
        return false;
    }

    let target_node = edge.target().node;
    if source_node_layer_constraint == LayerConstraint::Last
        && graph.layerless_nodes[target_node].kind() == LNodeKind::Label
    {
        return false;
    }

    graph.layerless_nodes[target_node].layer_constraint != LayerConstraint::LastSeparate
}

fn can_reverse_incoming_edge(
    graph: &LGraph,
    target_node_layer_constraint: LayerConstraint,
    edge: EdgeId,
) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    if edge.reversed {
        return false;
    }

    let source_node = edge.source().node;
    if target_node_layer_constraint == LayerConstraint::First
        && graph.layerless_nodes[source_node].kind() == LNodeKind::Label
    {
        return false;
    }

    graph.layerless_nodes[source_node].layer_constraint != LayerConstraint::FirstSeparate
}
