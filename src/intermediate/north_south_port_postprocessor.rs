//! Port of ELK's `NorthSouthPortPostprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/NorthSouthPortPostprocessor.java
//!
//! The spline branch of the post-processor waits for the spline router, which is not ported.

use crate::graph::{LGraph, LNodeKind, LPoint, PortRef};

/// Ports `NorthSouthPortPostprocessor.process`, without the spline branch.
///
/// Takes the dummy nodes out of their layers and gives each edge back to the port it came from.
/// Each edge gets a bend point at the height of the dummy, above or below the node, straight
/// over or under the port.
pub fn postprocess_north_south_ports(graph: &mut LGraph) {
    for layer_index in 0..graph.layers().len() {
        // A copy, because the loop takes nodes out of the layer.
        let node_array = graph.layers()[layer_index].nodes().to_vec();
        for node in node_array {
            if graph.layerless_nodes[node].kind() != LNodeKind::NorthSouthPort {
                continue;
            }

            // Whether the dummy has several ports and all of them stand for the same port.
            let ports = &graph.layerless_nodes[node].ports;
            let same_origin_port = ports.len() >= 2
                && ports
                    .ids()
                    .zip(ports.ids().skip(1))
                    .all(|(first, second)| ports[first].origin_port == ports[second].origin_port);

            // Moving an edge to the origin port never changes the dummy's list.
            for position in 0..graph.layerless_nodes[node].ports.len() {
                let Some(port) = graph.layerless_nodes[node].ports.id_at(position) else {
                    continue;
                };
                let dummy_port = PortRef { node, port };
                if !graph.layerless_nodes[node].ports[port]
                    .incoming_edges()
                    .is_empty()
                {
                    process_input_port(graph, dummy_port, same_origin_port);
                }
                if !graph.layerless_nodes[node].ports[port]
                    .outgoing_edges()
                    .is_empty()
                {
                    process_output_port(graph, dummy_port, same_origin_port);
                }
            }

            graph.remove_node_from_layer(node);
        }
    }
}

/// Ports `NorthSouthPortPostprocessor.processInputPort`: gives the edges that enter the dummy
/// port back to the port the dummy was made for.
///
/// `add_junction_points` is true for a dummy whose ports all stand for one port: its edges meet
/// at the bend point.
fn process_input_port(graph: &mut LGraph, input_port: PortRef, add_junction_points: bool) {
    let Some(origin_port) =
        graph.layerless_nodes[input_port.node].ports[input_port.port].origin_port
    else {
        return;
    };
    let x = absolute_anchor(graph, origin_port).x;
    let y = graph.layerless_nodes[input_port.node].position.y;

    let edges = graph.layerless_nodes[input_port.node].ports[input_port.port]
        .incoming_edges()
        .to_vec();
    for edge in edges {
        graph.set_edge_target(edge, origin_port);
        graph.edges[edge].bend_points.push(LPoint { x, y });
        if add_junction_points {
            graph.edges[edge].junction_points.push(LPoint { x, y });
        }
    }
}

/// Ports `NorthSouthPortPostprocessor.processOutputPort`: gives the edges that leave the dummy
/// port back to the port the dummy was made for.
fn process_output_port(graph: &mut LGraph, output_port: PortRef, add_junction_points: bool) {
    let Some(origin_port) =
        graph.layerless_nodes[output_port.node].ports[output_port.port].origin_port
    else {
        return;
    };
    let x = absolute_anchor(graph, origin_port).x;
    let y = graph.layerless_nodes[output_port.node].position.y;

    let edges = graph.layerless_nodes[output_port.node].ports[output_port.port]
        .outgoing_edges()
        .to_vec();
    for edge in edges {
        graph.set_edge_source(edge, origin_port);
        graph.edges[edge].bend_points.insert(0, LPoint { x, y });
        if add_junction_points {
            graph.edges[edge].junction_points.push(LPoint { x, y });
        }
    }
}

/// `LPort.getAbsoluteAnchor`: the node's position, the port's position and the port's anchor.
fn absolute_anchor(graph: &LGraph, port: PortRef) -> LPoint {
    let node = &graph.layerless_nodes[port.node];
    let data = &node.ports[port.port];
    LPoint {
        x: node.position.x + data.position.x + data.anchor.x,
        y: node.position.y + data.position.y + data.anchor.y,
    }
}
