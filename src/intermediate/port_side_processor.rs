//! Port of ELK's `PortSideProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PortSideProcessor.java

use crate::graph::{GraphId, LGraph, LNode, NodeId, OtherGraphs, PortRef, PortSide};
use crate::options::PortConstraints;

pub fn process_port_sides(graph: &mut LGraph, others: OtherGraphs<'_>) {
    let layerless_nodes = graph.node_ids().collect::<Vec<_>>();
    for node in layerless_nodes {
        process_node_port_sides(graph, others, node);
    }

    let layered_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();
    for node in layered_nodes {
        process_node_port_sides(graph, others, node);
    }
}

fn process_node_port_sides(graph: &mut LGraph, others: OtherGraphs<'_>, node: NodeId) {
    let side_fixed = graph.layerless_nodes[node].port_constraints.is_side_fixed();

    // `set_port_side` writes a side and never changes the list, so the places stay valid.
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        if !side_fixed || graph.layerless_nodes[node].ports[port].side == PortSide::Undefined {
            set_port_side(graph, others, PortRef { node, port });
        }
    }

    if !side_fixed {
        graph.layerless_nodes[node].port_constraints = PortConstraints::FixedSide;
    }
}

pub fn set_port_side(graph: &mut LGraph, others: OtherGraphs<'_>, port: PortRef) {
    let port_data = &graph.layerless_nodes[port.node].ports[port.port];
    let side = if let Some(dummy) = port_data.port_dummy.as_ref() {
        external_port_dummy_side(graph, others, port.node, dummy.graph_id, dummy.node)
    } else if port_data.net_flow() < 0 {
        PortSide::East
    } else {
        PortSide::West
    };
    if side != PortSide::Undefined {
        graph.layerless_nodes[port.node].ports[port.port].set_side(side);
    }
}

fn external_port_dummy_side(
    graph: &LGraph,
    others: OtherGraphs<'_>,
    port_node: NodeId,
    dummy_graph_id: GraphId,
    dummy_node: NodeId,
) -> PortSide {
    if dummy_graph_id == graph.id {
        return graph
            .layerless_nodes
            .get(dummy_node)
            .map_or(PortSide::Undefined, LNode::external_port_side);
    }

    // The dummy is in the graph inside the port's node, and no other graph.
    graph
        .layerless_nodes
        .get(port_node)
        .and_then(|node| node.nested_graph)
        .filter(|&nested| nested == dummy_graph_id)
        .and_then(|nested| others.get(nested))
        .and_then(|nested| nested.layerless_nodes.get(dummy_node))
        .map_or(PortSide::Undefined, LNode::external_port_side)
}
