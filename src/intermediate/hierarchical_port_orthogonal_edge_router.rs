//! Port of ELK's `HierarchicalPortOrthogonalEdgeRouter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalPortOrthogonalEdgeRouter.java

use crate::common::nodespacing;
use crate::graph::{
    LGraph, LNode, LPoint, LayeredEdge, NodeId, PortId, PortRef, PortSide, PortType,
};
use crate::intermediate::end_label_postprocessor::hand_back_end_labels;
use crate::intermediate::hierarchical_port_position_processor::{
    actual_graph_height, border_to_content_area_y, external_port_anchor,
};
use crate::intermediate::long_edge_joiner::first_port_on_side;
use crate::options::{PortConstraints, PortLabelPlacement, SizeConstraints};
use crate::phase::routing::orthogonal::{RoutingDirection, route_edges};

pub fn process_hierarchical_port_orthogonal_edges(graph: &mut LGraph) {
    if graph.layers().is_empty() {
        return;
    }

    let north_south_dummies = restore_north_south_dummies(graph);
    set_north_south_dummy_coordinates(graph, &north_south_dummies);
    route_north_south_dummy_edges(graph, &north_south_dummies);
    remove_temporary_north_south_dummies(graph);
    fix_hierarchical_port_coordinates(graph);
    correct_hierarchical_port_slanted_edge_segments(graph);
}

pub(crate) fn restore_north_south_dummies(graph: &mut LGraph) -> Vec<NodeId> {
    let restored_dummies = graph.replaced_external_port_dummies.clone();
    if restored_dummies.is_empty() {
        return restored_dummies;
    }

    for dummy in &restored_dummies {
        restore_north_south_dummy(graph, *dummy);
    }

    let replacement_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .filter(|node| is_temporary_replacement(&graph.layerless_nodes[*node]))
        .collect::<Vec<_>>();

    for replacement in replacement_nodes {
        if let Some(original) = graph.layerless_nodes[replacement]
            .external_port()
            .and_then(|external| external.replaced_dummy)
        {
            connect_replacement_dummy_to_original(graph, replacement, original);
        }
    }

    let last_layer = graph.layers().len() - 1;
    for dummy in &restored_dummies {
        graph.set_node_layer(*dummy, last_layer);
    }

    restored_dummies
}

fn restore_north_south_dummy(graph: &mut LGraph, dummy: NodeId) {
    let external_side = graph.layerless_nodes[dummy]
        .external_port()
        .map(|external| external.side);
    // Java: `dummy.getPorts().get(0)`, which throws on an empty list before the side is looked
    // at. An external port dummy always lists its port, so an empty list is a bug here, never
    // caller input.
    let port = graph.layerless_nodes[dummy]
        .ports
        .first_mut()
        .expect("a restored external port dummy lists at least one port");
    let Some(external_side) = external_side else {
        return;
    };

    port.side = match external_side {
        PortSide::North => PortSide::South,
        PortSide::South => PortSide::North,
        side => side,
    };
    port.port_type = PortType::Output;

    // Since the dummy was hidden from the algorithm, its port labels are not placed and its
    // margins are not set. That needs a fix if port labels count for the size of the node.
    if graph
        .options
        .node_size
        .constraints
        .contains(SizeConstraints::PORT_LABELS)
    {
        place_restored_dummy_labels(graph, dummy);
        nodespacing::calculate_node_margins(graph, [dummy]);
    }
}

/// Places the labels of a restored dummy: below the port under `INSIDE` port labels, beside it
/// under `OUTSIDE` ones.
///
/// Ports the label part of `HierarchicalPortOrthogonalEdgeRouter.restoreDummy`.
fn place_restored_dummy_labels(graph: &mut LGraph, dummy: NodeId) {
    let placement = graph.options.port_label_placement;
    let data = &mut graph.layerless_nodes[dummy];
    let Some(external) = data.external_port() else {
        return;
    };
    // The importer set the spacings on the dummy.
    let spacings = external.label_spacings;
    let dummy_width = data.size.width;
    let Some(port) = data.ports.first_mut() else {
        return;
    };
    if placement.contains(PortLabelPlacement::INSIDE) {
        let mut current_y = spacings.label_port_vertical;
        let x_centre_relative_to_port = dummy_width / 2.0 - port.position.x;
        for label in &mut port.labels {
            label.position.y = current_y;
            label.position.x = x_centre_relative_to_port - label.size.width / 2.0;
            current_y += label.size.height + spacings.label_label;
        }
    } else if placement.contains(PortLabelPlacement::OUTSIDE) {
        // The labels have no height, but they need an x coordinate.
        for label in &mut port.labels {
            label.position.x = spacings.label_port_horizontal + dummy_width - port.position.x;
        }
    }
}

pub(crate) fn connect_replacement_dummy_to_original(
    graph: &mut LGraph,
    replacement: NodeId,
    original: NodeId,
) {
    // Java: `original.getPorts().get(0)`, the first port of the list, which a reorder may have
    // moved away from the first port added. Java throws on an empty list. An external port dummy
    // always lists its port, so an empty list is a bug here, never caller input.
    let original_port = graph.layerless_nodes[original]
        .ports
        .id_at(0)
        .expect("a replaced external port dummy lists at least one port");
    let Some(external_side) = graph.layerless_nodes[replacement]
        .external_port()
        .map(|external| external.side)
    else {
        return;
    };
    if graph.layerless_nodes[replacement].ports.iter().any(|port| {
        port.side == external_side
            && port.outgoing_edges().iter().any(|edge| {
                graph.edges[*edge].target().node == original
                    && graph.edges[*edge].target().port == original_port
            })
    }) {
        return;
    }

    let Some(origin_port) = graph.add_port(
        replacement,
        PortType::Output,
        external_side,
        Default::default(),
    ) else {
        return;
    };

    let edge = {
        let mut built_edge = LayeredEdge::new(
            format!(
                "{}->{}:hierarchical-origin",
                graph.layerless_nodes[replacement].id, graph.layerless_nodes[original].id
            ),
            origin_port,
            PortRef {
                node: original,
                port: original_port,
            },
        );
        built_edge.source_node_id = graph.layerless_nodes[replacement].id.clone();
        built_edge.target_node_id = graph.layerless_nodes[original].id.clone();
        built_edge
    };
    graph.add_edge(edge);
}

fn set_north_south_dummy_coordinates(graph: &mut LGraph, north_south_dummies: &[NodeId]) {
    if north_south_dummies.is_empty() {
        return;
    }

    let graph_width = graph.size.width + graph.padding.left + graph.padding.right;
    let north_y = -graph.padding.top - graph.offset.y;
    let south_y = graph.size.height + graph.padding.top + graph.padding.bottom - graph.offset.y;
    let mut northern_dummies = Vec::new();
    let mut southern_dummies = Vec::new();

    for dummy in north_south_dummies {
        let Some(external) = graph.layerless_nodes[*dummy].external_port() else {
            continue;
        };
        let (side, ratio_or_position) = (external.side, external.ratio_or_position);
        match graph.options.port_constraints {
            PortConstraints::Free | PortConstraints::FixedSide | PortConstraints::FixedOrder => {
                calculate_north_south_dummy_position(graph, *dummy);
            }
            PortConstraints::FixedRatio => {
                graph.layerless_nodes[*dummy].position.x =
                    graph_width * ratio_or_position - external_port_anchor(graph, *dummy).x;
                border_to_content_area_x(graph, *dummy);
            }
            PortConstraints::FixedPos => {
                graph.layerless_nodes[*dummy].position.x =
                    ratio_or_position - external_port_anchor(graph, *dummy).x;
                border_to_content_area_x(graph, *dummy);
                let required_width = graph.layerless_nodes[*dummy].position.x
                    + graph.layerless_nodes[*dummy].size.width / 2.0;
                graph.size.width = graph.size.width.max(required_width);
            }
            PortConstraints::Undefined => {
                calculate_north_south_dummy_position(graph, *dummy);
            }
        }

        match side {
            PortSide::North => {
                graph.layerless_nodes[*dummy].position.y = north_y;
                northern_dummies.push(*dummy);
            }
            PortSide::South => {
                graph.layerless_nodes[*dummy].position.y = south_y;
                southern_dummies.push(*dummy);
            }
            PortSide::Undefined | PortSide::East | PortSide::West => {}
        }
    }

    match graph.options.port_constraints {
        PortConstraints::Free | PortConstraints::FixedSide | PortConstraints::Undefined => {
            ensure_unique_north_south_positions(graph, &mut northern_dummies);
            ensure_unique_north_south_positions(graph, &mut southern_dummies);
        }
        PortConstraints::FixedOrder => {
            restore_north_south_dummy_order(graph, &mut northern_dummies);
            restore_north_south_dummy_order(graph, &mut southern_dummies);
        }
        PortConstraints::FixedRatio | PortConstraints::FixedPos => {}
    }
}

fn calculate_north_south_dummy_position(graph: &mut LGraph, dummy: NodeId) {
    let Some(dummy_port) = graph.layerless_nodes[dummy].ports.first() else {
        graph.layerless_nodes[dummy].position.x = 0.0;
        return;
    };

    let connected_ports = dummy_port
        .incoming_edges()
        .iter()
        .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.source()))
        .chain(
            dummy_port
                .outgoing_edges()
                .iter()
                .filter_map(|edge| graph.edges.get(*edge).map(|edge| edge.target())),
        )
        .collect::<Vec<_>>();

    if connected_ports.is_empty() {
        graph.layerless_nodes[dummy].position.x = 0.0;
        return;
    }

    let pos_sum = connected_ports
        .iter()
        .map(|port_ref| absolute_port_anchor(graph, *port_ref).x)
        .sum::<f64>();
    graph.layerless_nodes[dummy].position.x =
        pos_sum / connected_ports.len() as f64 - external_port_anchor(graph, dummy).x;
}

fn border_to_content_area_x(graph: &mut LGraph, node: NodeId) {
    graph.layerless_nodes[node].position.x -= graph.padding.left + graph.offset.x;
}

fn ensure_unique_north_south_positions(graph: &mut LGraph, dummies: &mut [NodeId]) {
    dummies.sort_by(|left, right| {
        graph.layerless_nodes[*left]
            .position
            .x
            .total_cmp(&graph.layerless_nodes[*right].position.x)
    });
    assign_ascending_north_south_coordinates(graph, dummies);
}

fn restore_north_south_dummy_order(graph: &mut LGraph, dummies: &mut [NodeId]) {
    // The dummies are external port dummies, the only nodes that have a ratio or position.
    let ratio_or_position = |node: &NodeId| {
        graph.layerless_nodes[*node]
            .external_port()
            .map_or(0.0, |external| external.ratio_or_position)
    };
    dummies.sort_by(|left, right| ratio_or_position(left).total_cmp(&ratio_or_position(right)));
    assign_ascending_north_south_coordinates(graph, dummies);
}

fn assign_ascending_north_south_coordinates(graph: &mut LGraph, dummies: &[NodeId]) {
    let Some(first) = dummies.first() else {
        return;
    };

    let spacing = graph.options.spacing.port_port;
    let first_node = &graph.layerless_nodes[*first];
    let mut next_valid_coordinate =
        first_node.position.x + first_node.size.width + first_node.margin.right + spacing;

    for dummy in dummies.iter().skip(1) {
        let current = &graph.layerless_nodes[*dummy];
        let delta = current.position.x - current.margin.left - next_valid_coordinate;
        if delta < 0.0 {
            graph.layerless_nodes[*dummy].position.x -= delta;
        }

        let current = &graph.layerless_nodes[*dummy];
        graph.size.width = graph
            .size
            .width
            .max(current.position.x + current.size.width);
        next_valid_coordinate =
            current.position.x + current.size.width + current.margin.right + spacing;
    }
}

fn route_north_south_dummy_edges(graph: &mut LGraph, north_south_dummies: &[NodeId]) {
    let mut northern_source_layer = Vec::new();
    let mut northern_target_layer = Vec::new();
    let mut southern_source_layer = Vec::new();
    let mut southern_target_layer = Vec::new();

    for dummy in north_south_dummies {
        let Some(side) = graph.layerless_nodes[*dummy]
            .external_port()
            .map(|external| external.side)
        else {
            continue;
        };
        match side {
            PortSide::North => {
                push_unique(&mut northern_target_layer, *dummy);
                for edge in graph.node_incoming_edges(*dummy) {
                    push_unique(&mut northern_source_layer, graph.edges[edge].source().node);
                }
            }
            PortSide::South => {
                push_unique(&mut southern_target_layer, *dummy);
                for edge in graph.node_incoming_edges(*dummy) {
                    push_unique(&mut southern_source_layer, graph.edges[edge].source().node);
                }
            }
            PortSide::Undefined | PortSide::East | PortSide::West => {}
        }
    }

    let node_spacing = graph.options.spacing.node_node;
    let edge_spacing = graph.options.spacing.edge_edge;

    if !northern_source_layer.is_empty() {
        let slots = route_edges(
            graph,
            RoutingDirection::SouthToNorth,
            Some(&northern_source_layer),
            Some(&northern_target_layer),
            -node_spacing - graph.offset.y,
            edge_spacing,
        );
        if slots > 0 {
            let routing_height = node_spacing + slots.saturating_sub(1) as f64 * edge_spacing;
            graph.offset.y += routing_height;
            graph.size.height += routing_height;
        }
    }

    if !southern_source_layer.is_empty() {
        let slots = route_edges(
            graph,
            RoutingDirection::NorthToSouth,
            Some(&southern_source_layer),
            Some(&southern_target_layer),
            graph.size.height + node_spacing - graph.offset.y,
            edge_spacing,
        );
        if slots > 0 {
            graph.size.height += node_spacing + slots.saturating_sub(1) as f64 * edge_spacing;
        }
    }
}

fn push_unique(nodes: &mut Vec<NodeId>, node: NodeId) {
    if !nodes.contains(&node) {
        nodes.push(node);
    }
}

pub(crate) fn remove_temporary_north_south_dummies(graph: &mut LGraph) {
    let temporary_nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .filter(|node| is_temporary_replacement(&graph.layerless_nodes[*node]))
        .collect::<Vec<_>>();

    for node in &temporary_nodes {
        remove_temporary_north_south_dummy(graph, *node);
    }

    for node in temporary_nodes {
        graph.remove_node_from_layer(node);
    }
}

/// Whether `node` is a temporary replacement of a north or south external port dummy, as
/// `HierarchicalPortConstraintProcessor` makes it.
fn is_temporary_replacement(node: &LNode) -> bool {
    node.external_port()
        .is_some_and(|external| external.replaced_dummy.is_some())
}

/// The temporary replacements that [`remove_temporary_north_south_dummies`] took out of their
/// layers and cut loose from every edge, for the caller to drop from the graph. Their end labels
/// go back to their edges first, unplaced, as the end label postprocessing never sees these nodes
/// in Java.
///
/// A replacement that still has an edge stays: one of the early returns of
/// [`remove_temporary_north_south_dummy`] left it connected, and dropping it would break the
/// edge. It then stays in the node list, as it did before this step dropped any.
pub(crate) fn release_temporary_north_south_dummies(graph: &mut LGraph) -> Vec<NodeId> {
    let released = graph
        .nodes_with_ids()
        .filter(|(_, node)| {
            is_temporary_replacement(node)
                && node.layer_index().is_none()
                && node.ports.iter().all(|port| {
                    port.incoming_edges().is_empty() && port.outgoing_edges().is_empty()
                })
        })
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    for &node in &released {
        for port in graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>() {
            hand_back_end_labels(graph, node, port);
        }
    }
    released
}

pub(crate) fn remove_temporary_north_south_dummy(graph: &mut LGraph, node: NodeId) {
    let Some(node_in_port) = first_port_on_side(graph, node, PortSide::West) else {
        return;
    };
    let Some(node_out_port) = first_port_on_side(graph, node, PortSide::East) else {
        return;
    };
    let Some(node_origin_port) = graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .find(|(_, port)| !matches!(port.side, PortSide::West | PortSide::East))
        .map(|(id, _)| id)
    else {
        return;
    };
    let Some(node_to_origin_edge) = graph.layerless_nodes[node].ports[node_origin_port]
        .outgoing_edges()
        .first()
        .copied()
    else {
        return;
    };
    let Some(replaced_dummy) = graph.layerless_nodes[node]
        .external_port()
        .and_then(|external| external.replaced_dummy)
    else {
        return;
    };

    let mut incoming_edge_bend_points = graph.edges[node_to_origin_edge].bend_points.clone();
    incoming_edge_bend_points.insert(
        0,
        node_relative_port_position(graph, node, node_origin_port),
    );

    let mut outgoing_edge_bend_points = graph.edges[node_to_origin_edge].bend_points.clone();
    outgoing_edge_bend_points.reverse();
    outgoing_edge_bend_points.push(node_relative_port_position(graph, node, node_origin_port));

    // Java: `replacedDummy.getPorts().get(0)`, the first port of the list. Java throws on an empty
    // list. An external port dummy always lists its port, so an empty list is a bug here, never
    // caller input.
    let replaced_dummy_first_port = graph.layerless_nodes[replaced_dummy]
        .ports
        .id_at(0)
        .expect("a replaced external port dummy lists at least one port");
    let replaced_dummy_port = PortRef {
        node: replaced_dummy,
        port: replaced_dummy_first_port,
    };

    let incoming_edges = graph.layerless_nodes[node].ports[node_in_port]
        .incoming_edges()
        .to_vec();
    for edge in incoming_edges {
        graph.set_edge_target(edge, replaced_dummy_port);
        graph.edges[edge]
            .bend_points
            .extend(incoming_edge_bend_points.iter().copied());
    }

    let outgoing_edges = graph.layerless_nodes[node].ports[node_out_port]
        .outgoing_edges()
        .to_vec();
    for edge in outgoing_edges {
        graph.set_edge_source(edge, replaced_dummy_port);
        graph.edges[edge]
            .bend_points
            .splice(0..0, outgoing_edge_bend_points.iter().copied());
    }

    graph.detach_edge(node_to_origin_edge);
}

fn node_relative_port_position(graph: &LGraph, node: NodeId, port: PortId) -> LPoint {
    LPoint {
        x: graph.layerless_nodes[node].position.x
            + graph.layerless_nodes[node].ports[port].position.x,
        y: graph.layerless_nodes[node].position.y
            + graph.layerless_nodes[node].ports[port].position.y,
    }
}

fn fix_hierarchical_port_coordinates(graph: &mut LGraph) {
    fix_hierarchical_port_coordinates_for_layer(graph, 0);
    fix_hierarchical_port_coordinates_for_layer(graph, graph.layers().len() - 1);
}

fn fix_hierarchical_port_coordinates_for_layer(graph: &mut LGraph, layer_index: usize) {
    let nodes = graph.layers()[layer_index].nodes().to_vec();
    let graph_actual_height = actual_graph_height(graph);
    let mut new_actual_graph_height = graph_actual_height;

    for node in &nodes {
        let Some(side) = graph.layerless_nodes[*node]
            .external_port()
            .map(|external| external.side)
        else {
            continue;
        };

        match side {
            PortSide::East => {
                graph.layerless_nodes[*node].position.x =
                    graph.size.width + graph.padding.right - graph.offset.x;
            }
            PortSide::West => {
                graph.layerless_nodes[*node].position.x = -graph.offset.x - graph.padding.left;
            }
            PortSide::North | PortSide::South | PortSide::Undefined => {}
        }

        if matches!(side, PortSide::East | PortSide::West) {
            let required_actual_graph_height =
                fix_hierarchical_port_east_west_y_coordinate(graph, *node, graph_actual_height);
            new_actual_graph_height = new_actual_graph_height.max(required_actual_graph_height);
        }
    }

    graph.size.height += new_actual_graph_height - graph_actual_height;

    for node in nodes {
        let Some(side) = graph.layerless_nodes[node]
            .external_port()
            .map(|external| external.side)
        else {
            continue;
        };

        match side {
            PortSide::North => {
                graph.layerless_nodes[node].position.y = -graph.offset.y - graph.padding.top;
            }
            PortSide::South => {
                graph.layerless_nodes[node].position.y =
                    graph.size.height + graph.padding.bottom - graph.offset.y;
            }
            PortSide::East | PortSide::West | PortSide::Undefined => {}
        }
    }
}

fn fix_hierarchical_port_east_west_y_coordinate(
    graph: &mut LGraph,
    node: NodeId,
    graph_actual_height: f64,
) -> f64 {
    let mut required_actual_graph_height = 0.0;
    let Some(external) = graph.layerless_nodes[node].external_port() else {
        return required_actual_graph_height;
    };
    let (ratio_or_position, external_height) = (external.ratio_or_position, external.size.height);

    if graph.options.port_constraints.is_ratio_fixed() {
        graph.layerless_nodes[node].position.y =
            graph_actual_height * ratio_or_position - external_port_anchor(graph, node).y;
        required_actual_graph_height = graph.layerless_nodes[node].position.y + external_height;
        border_to_content_area_y(graph, node);
    } else if graph.options.port_constraints.is_pos_fixed() {
        graph.layerless_nodes[node].position.y =
            ratio_or_position - external_port_anchor(graph, node).y;
        required_actual_graph_height = graph.layerless_nodes[node].position.y + external_height;
        border_to_content_area_y(graph, node);
    }

    required_actual_graph_height
}

fn correct_hierarchical_port_slanted_edge_segments(graph: &mut LGraph) {
    correct_hierarchical_port_slanted_edge_segments_for_layer(graph, 0);
    correct_hierarchical_port_slanted_edge_segments_for_layer(graph, graph.layers().len() - 1);
}

fn correct_hierarchical_port_slanted_edge_segments_for_layer(
    graph: &mut LGraph,
    layer_index: usize,
) {
    let nodes = graph.layers()[layer_index].nodes().to_vec();

    for node in nodes {
        if !graph.layerless_nodes[node]
            .external_port()
            .is_some_and(|external| matches!(external.side, PortSide::East | PortSide::West))
        {
            continue;
        }

        let connected_edges = graph.node_connected_edges(node);
        for edge in connected_edges {
            if graph.edges[edge].bend_points.is_empty() {
                continue;
            }

            let source = graph.edges[edge].source();
            if source.node == node {
                let source_y = absolute_port_anchor(graph, source).y;
                if let Some(first) = graph.edges[edge].bend_points.first_mut() {
                    first.y = source_y;
                }
            }

            let target = graph.edges[edge].target();
            if target.node == node {
                let target_y = absolute_port_anchor(graph, target).y;
                if let Some(last) = graph.edges[edge].bend_points.last_mut() {
                    last.y = target_y;
                }
            }
        }
    }
}

fn absolute_port_anchor(graph: &LGraph, port_ref: PortRef) -> LPoint {
    let node = &graph.layerless_nodes[port_ref.node];
    let port = &node.ports[port_ref.port];
    LPoint {
        x: node.position.x + port.position.x + port.anchor.x,
        y: node.position.y + port.position.y + port.anchor.y,
    }
}
