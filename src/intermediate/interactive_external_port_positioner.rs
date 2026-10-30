//! Port of ELK's `InteractiveExternalPortPositioner`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/InteractiveExternalPortPositioner.java

use crate::graph::{LGraph, LNodeKind, NodeId};
use crate::options::LayerConstraint;

const INTERACTIVE_EXTERNAL_PORT_SPACING: f64 = 10.0;

pub fn position_interactive_external_ports(graph: &mut LGraph) {
    if !graph.graph_properties.external_ports {
        return;
    }

    let Some(bounds) = interactive_normal_node_bounds(graph) else {
        return;
    };

    for node_index in graph.node_ids() {
        if graph.layerless_nodes[node_index].kind() != LNodeKind::ExternalPort {
            continue;
        }

        if graph.layerless_nodes[node_index].layer_constraint() == LayerConstraint::FirstSeparate {
            graph.layerless_nodes[node_index].position.x =
                bounds.min_x - INTERACTIVE_EXTERNAL_PORT_SPACING;
            if let Some(y) = first_connected_other_node_center_y(graph, node_index, true) {
                graph.layerless_nodes[node_index].position.y = y;
            }
            continue;
        }

        if graph.layerless_nodes[node_index].layer_constraint() == LayerConstraint::LastSeparate {
            graph.layerless_nodes[node_index].position.x =
                bounds.max_x + INTERACTIVE_EXTERNAL_PORT_SPACING;
            if let Some(y) = first_connected_other_node_center_y(graph, node_index, false) {
                graph.layerless_nodes[node_index].position.y = y;
            }
            continue;
        }

        match graph.layerless_nodes[node_index].in_layer_constraint() {
            crate::graph::InLayerConstraint::Top => {
                if let Some(x) = north_south_external_port_x(graph, node_index) {
                    graph.layerless_nodes[node_index].position.x =
                        x + INTERACTIVE_EXTERNAL_PORT_SPACING;
                }
                graph.layerless_nodes[node_index].position.y =
                    bounds.min_y - INTERACTIVE_EXTERNAL_PORT_SPACING;
            }
            crate::graph::InLayerConstraint::Bottom => {
                if let Some(x) = north_south_external_port_x(graph, node_index) {
                    graph.layerless_nodes[node_index].position.x =
                        x + INTERACTIVE_EXTERNAL_PORT_SPACING;
                }
                graph.layerless_nodes[node_index].position.y =
                    bounds.max_y + INTERACTIVE_EXTERNAL_PORT_SPACING;
            }
            crate::graph::InLayerConstraint::None => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct InteractiveBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

fn interactive_normal_node_bounds(graph: &LGraph) -> Option<InteractiveBounds> {
    let mut bounds: Option<InteractiveBounds> = None;
    for node in graph
        .layerless_nodes
        .iter()
        .filter(|node| node.kind() == LNodeKind::Normal)
    {
        let current = InteractiveBounds {
            min_x: node.position.x - node.margin.left,
            max_x: node.position.x + node.size.width + node.margin.right,
            min_y: node.position.y - node.margin.top,
            max_y: node.position.y + node.size.height + node.margin.bottom,
        };
        bounds = Some(match bounds {
            None => current,
            Some(previous) => InteractiveBounds {
                min_x: previous.min_x.min(current.min_x),
                max_x: previous.max_x.max(current.max_x),
                min_y: previous.min_y.min(current.min_y),
                max_y: previous.max_y.max(current.max_y),
            },
        });
    }
    bounds
}

fn first_connected_other_node_center_y(
    graph: &LGraph,
    dummy_index: NodeId,
    other_is_target: bool,
) -> Option<f64> {
    if let Some(edge_index) = graph.node_connected_edges(dummy_index).into_iter().next() {
        let edge = &graph.edges[edge_index];
        let other = if other_is_target {
            edge.target().node
        } else {
            edge.source().node
        };
        let node = graph.layerless_nodes.get(other)?;
        Some(node.position.y + node.size.height / 2.0)
    } else {
        None
    }
}

fn north_south_external_port_x(graph: &LGraph, dummy_index: NodeId) -> Option<f64> {
    let port = graph.layerless_nodes.get(dummy_index)?.ports.first()?;
    if !port.outgoing_edges().is_empty() && !port.incoming_edges().is_empty() {
        return None;
    }

    if !port.outgoing_edges().is_empty() {
        return port
            .outgoing_edges()
            .iter()
            .filter_map(|edge_index| graph.edges.get(*edge_index))
            .filter_map(|edge| graph.layerless_nodes.get(edge.target().node))
            .map(|node| node.position.x - node.margin.left)
            .reduce(f64::min);
    }

    if !port.incoming_edges().is_empty() {
        return port
            .incoming_edges()
            .iter()
            .filter_map(|edge_index| graph.edges.get(*edge_index))
            .filter_map(|edge| graph.layerless_nodes.get(edge.source().node))
            .map(|node| node.position.x + node.size.width + node.margin.right)
            .reduce(f64::max);
    }

    None
}
