//! Port of ELK's `InvertedPortProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/InvertedPortProcessor.java

use crate::graph::{
    DummyData, EdgeId, LGraph, LNode, LNodeKind, LPoint, LayeredEdge, LongEdgeData, NodeId, PortId,
    PortRef, PortSide, PortType,
};
use crate::intermediate::long_edge_splitter::move_head_labels;
use crate::options::PortConstraints;

pub fn process_inverted_ports(graph: &mut LGraph) {
    let mut previous_layer = None;
    let mut unassigned_nodes = Vec::new();

    for layer_index in 0..graph.layers().len() {
        if let Some(target_layer) = previous_layer {
            assign_nodes_to_layer(graph, &mut unassigned_nodes, target_layer);
        }
        previous_layer = Some(layer_index);

        let layer_nodes = graph.layers()[layer_index].nodes().to_vec();
        for node in layer_nodes {
            if graph.layerless_nodes[node].kind() != LNodeKind::Normal
                || !graph.layerless_nodes[node].port_constraints.is_side_fixed()
            {
                continue;
            }

            let east_input_ports = ports_with_incidence(graph, node, PortSide::East, true);
            for port in east_input_ports {
                let incoming_edges = graph.layerless_nodes[node].ports[port]
                    .incoming_edges()
                    .to_vec();
                for edge in incoming_edges {
                    create_east_port_side_dummy(
                        graph,
                        PortRef { node, port },
                        edge,
                        &mut unassigned_nodes,
                    );
                }
            }

            let west_output_ports = ports_with_incidence(graph, node, PortSide::West, false);
            for port in west_output_ports {
                let outgoing_edges = graph.layerless_nodes[node].ports[port]
                    .outgoing_edges()
                    .to_vec();
                for edge in outgoing_edges {
                    create_west_port_side_dummy(
                        graph,
                        PortRef { node, port },
                        edge,
                        &mut unassigned_nodes,
                    );
                }
            }
        }
    }

    if let Some(target_layer) = previous_layer {
        assign_nodes_to_layer(graph, &mut unassigned_nodes, target_layer);
    }
}

fn ports_with_incidence(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
    incoming: bool,
) -> Vec<PortId> {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter_map(|(port, data)| {
            (data.side == side
                && if incoming {
                    !data.incoming_edges().is_empty()
                } else {
                    !data.outgoing_edges().is_empty()
                })
            .then_some(port)
        })
        .collect()
}

fn create_east_port_side_dummy(
    graph: &mut LGraph,
    eastward_port: PortRef,
    edge_index: EdgeId,
    unassigned_nodes: &mut Vec<NodeId>,
) {
    if graph.edges[edge_index].source().node == eastward_port.node {
        return;
    }

    let original_target = graph.edges[edge_index].target();
    let dummy = create_inverted_port_dummy(graph, edge_index);
    unassigned_nodes.push(dummy);
    let dummy_input = add_inverted_dummy_port(graph, dummy, PortType::Input, PortSide::West);
    let dummy_output = add_inverted_dummy_port(graph, dummy, PortType::Output, PortSide::East);

    if !graph.set_edge_target(edge_index, dummy_input) {
        return;
    }

    let dummy_edge = clone_inverted_dummy_edge(graph, edge_index, dummy_output, original_target);
    let Some(dummy_edge_index) = graph.add_edge(dummy_edge) else {
        return;
    };

    set_inverted_port_dummy_long_edge_ports(graph, dummy, edge_index, dummy_edge_index);
    move_head_labels(graph, edge_index, dummy_edge_index);
}

fn create_west_port_side_dummy(
    graph: &mut LGraph,
    westward_port: PortRef,
    edge_index: EdgeId,
    unassigned_nodes: &mut Vec<NodeId>,
) {
    if graph.edges[edge_index].target().node == westward_port.node {
        return;
    }

    let original_target = graph.edges[edge_index].target();
    let dummy = create_inverted_port_dummy(graph, edge_index);
    unassigned_nodes.push(dummy);
    let dummy_input = add_inverted_dummy_port(graph, dummy, PortType::Input, PortSide::West);
    let dummy_output = add_inverted_dummy_port(graph, dummy, PortType::Output, PortSide::East);

    if !graph.set_edge_target(edge_index, dummy_input) {
        return;
    }

    let dummy_edge = clone_inverted_dummy_edge(graph, edge_index, dummy_output, original_target);
    let Some(dummy_edge_index) = graph.add_edge(dummy_edge) else {
        return;
    };

    set_inverted_port_dummy_long_edge_ports(graph, dummy, edge_index, dummy_edge_index);
    move_head_labels(graph, edge_index, dummy_edge_index);
}

// Java: InvertedPortProcessor.setLongEdgeSourceAndTarget. This is not LongEdgeSplitter's
// setDummyNodeProperties: only a neighbouring long edge dummy passes its ends on. A label dummy
// neighbour is not copied from. The dummy then names that label dummy's own port, so the label
// side selector sees a different long edge than the label dummy's. It also leaves
// long_edge_has_label_dummies alone.
fn set_inverted_port_dummy_long_edge_ports(
    graph: &mut LGraph,
    dummy_node: NodeId,
    in_edge: EdgeId,
    out_edge: EdgeId,
) {
    let source_port = graph.edges[in_edge].source();
    let target_port = graph.edges[out_edge].target();

    let source = if graph.layerless_nodes[source_port.node].kind() == LNodeKind::LongEdge {
        graph.layerless_nodes[source_port.node]
            .long_edge_ends()
            .and_then(|ends| ends.source)
    } else {
        Some(source_port)
    };
    let target = if graph.layerless_nodes[target_port.node].kind() == LNodeKind::LongEdge {
        graph.layerless_nodes[target_port.node]
            .long_edge_ends()
            .and_then(|ends| ends.target)
    } else {
        Some(target_port)
    };
    if let Some(ends) = graph.layerless_nodes[dummy_node].long_edge_ends_mut() {
        ends.source = source;
        ends.target = target;
    }
}

fn create_inverted_port_dummy(graph: &mut LGraph, edge_index: EdgeId) -> NodeId {
    graph.push_node_with(|id| {
        let mut dummy = LNode::new(
            format!("invertedPort:{}:{}", edge_index.index(), id.index()),
            0.0,
            0.0,
            None,
        );
        dummy.set_dummy(DummyData::LongEdge(LongEdgeData {
            origin_edge: Some(edge_index),
            ..LongEdgeData::default()
        }));
        dummy.port_constraints = PortConstraints::FixedPos;
        dummy
    })
}

fn add_inverted_dummy_port(
    graph: &mut LGraph,
    dummy: NodeId,
    port_type: PortType,
    side: PortSide,
) -> PortRef {
    graph
        .add_port(dummy, port_type, side, LPoint::default())
        .expect("inverted-port dummy was just inserted")
}

fn clone_inverted_dummy_edge(
    graph: &LGraph,
    edge_index: EdgeId,
    source: PortRef,
    target: PortRef,
) -> LayeredEdge {
    LayeredEdge::segment_of(&graph.edges[edge_index], source, target)
}

fn assign_nodes_to_layer(graph: &mut LGraph, nodes: &mut Vec<NodeId>, layer_index: usize) {
    for node in nodes.drain(..) {
        graph.set_node_layer(node, layer_index);
    }
}
