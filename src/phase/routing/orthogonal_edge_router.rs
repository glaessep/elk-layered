//! Port of ELK's `OrthogonalEdgeRouter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/OrthogonalEdgeRouter.java

use crate::graph::{LGraph, NodeId};
use crate::options::Alignment;
use crate::phase::routing::orthogonal;

pub fn route_edges_orthogonal(graph: &mut LGraph) {
    let node_node_spacing = graph.options.spacing.node_node_between_layers;
    let edge_edge_spacing = graph.options.spacing.edge_edge_between_layers;
    let edge_node_spacing = graph.options.spacing.edge_node_between_layers;

    let mut xpos = 0.0;
    let mut left_layer_nodes: Option<Vec<NodeId>> = None;

    for layer_index in 0..=graph.layers().len() {
        let right_layer_nodes = graph
            .layers()
            .get(layer_index)
            .map(|layer| layer.nodes().to_vec());

        if let Some(nodes) = left_layer_nodes.as_ref() {
            place_nodes_horizontally(graph, nodes, xpos);
            xpos += graph.layers()[layer_index - 1].size.width;
        }

        let start_pos = if left_layer_nodes.is_none() {
            xpos
        } else {
            xpos + edge_node_spacing
        };
        let slots_count = orthogonal::route_edges_west_to_east(
            graph,
            left_layer_nodes.as_deref(),
            right_layer_nodes.as_deref(),
            start_pos,
            edge_edge_spacing,
        );

        let is_left_layer_external = left_layer_nodes
            .as_deref()
            .map(|nodes| {
                nodes
                    .iter()
                    .all(|node| is_external_west_or_east_port(graph, *node))
            })
            .unwrap_or(true);
        let is_right_layer_external = right_layer_nodes
            .as_deref()
            .map(|nodes| {
                nodes
                    .iter()
                    .all(|node| is_external_west_or_east_port(graph, *node))
            })
            .unwrap_or(true);

        if slots_count > 0 {
            let mut routing_width = (slots_count - 1) as f64 * edge_edge_spacing;
            if left_layer_nodes.is_some() {
                routing_width += edge_node_spacing;
            }
            if right_layer_nodes.is_some() {
                routing_width += edge_node_spacing;
            }
            if routing_width < node_node_spacing
                && !is_left_layer_external
                && !is_right_layer_external
            {
                routing_width = node_node_spacing;
            }
            xpos += routing_width;
        } else if !is_left_layer_external && !is_right_layer_external {
            xpos += node_node_spacing;
        }

        left_layer_nodes = right_layer_nodes;
    }

    graph.size.width = xpos;
}

fn place_nodes_horizontally(graph: &mut LGraph, nodes: &[NodeId], xoffset: f64) {
    let max_left_margin = nodes
        .iter()
        .map(|node| graph.layerless_nodes[*node].margin.left)
        .fold(0.0, f64::max);
    let max_right_margin = nodes
        .iter()
        .map(|node| graph.layerless_nodes[*node].margin.right)
        .fold(0.0, f64::max);
    let layer_width = nodes
        .iter()
        .filter_map(|node| graph.layerless_nodes[*node].layer_index())
        .next()
        .and_then(|layer| graph.layers().get(layer))
        .map(|layer| layer.size.width)
        .unwrap_or(0.0);

    for node in nodes {
        let lnode = &graph.layerless_nodes[*node];
        let ratio = horizontal_alignment_ratio(graph, *node);

        let node_width = lnode.size.width;
        let mut xpos = (layer_width - node_width) * ratio;
        if ratio > 0.5 {
            xpos -= max_right_margin * 2.0 * (ratio - 0.5);
        } else if ratio < 0.5 {
            xpos += max_left_margin * 2.0 * (0.5 - ratio);
        }

        xpos = xpos.max(lnode.margin.left);
        xpos = xpos.min(layer_width - lnode.margin.right - node_width);
        graph.layerless_nodes[*node].position.x = xoffset + xpos;
    }
}

fn horizontal_alignment_ratio(graph: &LGraph, node: NodeId) -> f64 {
    let lnode = &graph.layerless_nodes[node];
    match lnode.node_alignment {
        Alignment::Left => 0.0,
        Alignment::Right => 1.0,
        Alignment::Center => 0.5,
        Alignment::Automatic | Alignment::Top | Alignment::Bottom => {
            let inports = lnode
                .ports
                .iter()
                .filter(|port| !port.incoming_edges().is_empty())
                .count();
            let outports = lnode
                .ports
                .iter()
                .filter(|port| !port.outgoing_edges().is_empty())
                .count();
            if inports + outports == 0 {
                0.5
            } else {
                outports as f64 / (inports + outports) as f64
            }
        }
    }
}

fn is_external_west_or_east_port(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node]
        .external_port()
        .is_some_and(|external| {
            matches!(
                external.side,
                crate::graph::PortSide::West | crate::graph::PortSide::East
            )
        })
}
