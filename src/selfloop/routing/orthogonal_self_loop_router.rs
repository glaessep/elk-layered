//! Port of ELK's `OrthogonalSelfLoopRouter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/routing/OrthogonalSelfLoopRouter.java

use crate::graph::{
    LGraph, LMargin, LPoint, NodeId, PortId, PortSide, SelfHyperLoop, SelfHyperLoopLabels,
    SelfLoopEdge, SelfLoopHolder,
};
use crate::selfloop::routing::routing_director::PortPlaces;
use crate::selfloop::self_hyper_loop_labels::write_self_loop_label_positions;

pub(crate) fn route_self_loop_holder(
    graph: &mut LGraph,
    holder: &SelfLoopHolder,
    places: &PortPlaces,
) {
    let mut new_margins = graph.layerless_nodes[holder.node].margin;
    let routing_slot_positions = compute_routing_slot_positions(graph, holder);

    for hyper_loop in &holder.hyper_loops {
        for sl_edge in &hyper_loop.edges {
            let direction =
                compute_edge_routing_direction(graph, places, holder.node, hyper_loop, sl_edge);
            let bend_points = compute_orthogonal_bend_points(
                graph,
                holder.node,
                hyper_loop,
                sl_edge,
                direction,
                &routing_slot_positions,
            );
            graph.edges[sl_edge.edge].bend_points = bend_points;
            for point in graph.edges[sl_edge.edge].bend_points.iter().copied() {
                update_node_margins_for_bend_point(graph, holder.node, &mut new_margins, point);
            }
        }
        if let Some(mut labels) = hyper_loop.labels.clone() {
            place_labels_on_routing_slot(graph, hyper_loop, &mut labels, &routing_slot_positions);
            update_node_margins_for_label(graph, holder.node, &mut new_margins, &labels);
            write_self_loop_label_positions(graph, &labels);
        }
    }

    graph.layerless_nodes[holder.node].margin = new_margins;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeRoutingDirection {
    Clockwise,
    CounterClockwise,
}

fn compute_edge_routing_direction(
    graph: &LGraph,
    places: &PortPlaces,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    sl_edge: &SelfLoopEdge,
) -> EdgeRoutingDirection {
    let source_side = graph.layerless_nodes[node].ports[sl_edge.source_port].side;
    let target_side = graph.layerless_nodes[node].ports[sl_edge.target_port].side;

    if source_side == target_side {
        if places.of(sl_edge.source_port) < places.of(sl_edge.target_port) {
            EdgeRoutingDirection::Clockwise
        } else {
            EdgeRoutingDirection::CounterClockwise
        }
    } else if source_side.right() == target_side {
        EdgeRoutingDirection::Clockwise
    } else if source_side.left() == target_side {
        EdgeRoutingDirection::CounterClockwise
    } else if hyper_loop.occupied_sides.contains(&source_side.right()) {
        EdgeRoutingDirection::Clockwise
    } else {
        EdgeRoutingDirection::CounterClockwise
    }
}

fn compute_routing_slot_positions(graph: &LGraph, holder: &SelfLoopHolder) -> [Vec<f64>; 5] {
    let mut slot_count = [0usize; 5];
    for hyper_loop in &holder.hyper_loops {
        for side in &hyper_loop.occupied_sides {
            let ordinal = side.ordinal();
            slot_count[ordinal] = slot_count[ordinal].max(hyper_loop.routing_slots[ordinal] + 1);
        }
    }

    let mut positions: [Vec<f64>; 5] = std::array::from_fn(|side_index| {
        let Some(_side) = side_from_ordinal(side_index) else {
            return Vec::new();
        };

        Vec::with_capacity(slot_count[side_index])
    });

    initialize_routing_slot_positions_with_max_label_height(
        &mut positions,
        holder,
        PortSide::North,
    );
    initialize_routing_slot_positions_with_max_label_height(
        &mut positions,
        holder,
        PortSide::South,
    );

    for side in [
        PortSide::North,
        PortSide::East,
        PortSide::South,
        PortSide::West,
    ] {
        let side_index = side.ordinal();
        let mut side_positions = std::mem::take(&mut positions[side_index]);
        let mut current = compute_baseline_position(graph, holder.node, side);
        let factor = if matches!(side, PortSide::North | PortSide::West) {
            -1.0
        } else {
            1.0
        };

        for slot in 0..slot_count[side_index] {
            let mut largest_label_size = side_positions.get(slot).copied().unwrap_or(0.0);
            if largest_label_size > 0.0 {
                largest_label_size += graph.options.spacing.edge_label;
            }
            if slot < side_positions.len() {
                side_positions[slot] = current;
            } else {
                side_positions.push(current);
            }
            current += factor * (largest_label_size + graph.options.spacing.edge_edge);
        }

        positions[side_index] = side_positions;
    }

    positions
}

fn initialize_routing_slot_positions_with_max_label_height(
    positions: &mut [Vec<f64>; 5],
    holder: &SelfLoopHolder,
    side: PortSide,
) {
    for hyper_loop in &holder.hyper_loops {
        let Some(labels) = &hyper_loop.labels else {
            continue;
        };
        if labels.side != Some(side) {
            continue;
        }
        let slot = hyper_loop.routing_slots[side.ordinal()];
        let side_positions = &mut positions[side.ordinal()];
        while side_positions.len() <= slot {
            side_positions.push(0.0);
        }
        side_positions[slot] = side_positions[slot].max(labels.size.height);
    }
}

fn side_from_ordinal(ordinal: usize) -> Option<PortSide> {
    match ordinal {
        1 => Some(PortSide::North),
        2 => Some(PortSide::East),
        3 => Some(PortSide::South),
        4 => Some(PortSide::West),
        _ => None,
    }
}

fn compute_baseline_position(graph: &LGraph, node: NodeId, side: PortSide) -> f64 {
    let lnode = &graph.layerless_nodes[node];
    let node_self_loop_distance = graph.options.spacing.node_self_loop;
    match side {
        PortSide::North => -lnode.margin.top - node_self_loop_distance,
        PortSide::East => lnode.size.width + lnode.margin.right + node_self_loop_distance,
        PortSide::South => lnode.size.height + lnode.margin.bottom + node_self_loop_distance,
        PortSide::West => -lnode.margin.left - node_self_loop_distance,
        PortSide::Undefined => 0.0,
    }
}

fn compute_orthogonal_bend_points(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    sl_edge: &SelfLoopEdge,
    routing_direction: EdgeRoutingDirection,
    routing_slot_positions: &[Vec<f64>; 5],
) -> Vec<LPoint> {
    let mut bend_points = Vec::new();

    add_outer_bend_point(
        graph,
        node,
        hyper_loop,
        sl_edge.source_port,
        routing_slot_positions,
        &mut bend_points,
    );
    add_corner_bend_points(
        graph,
        node,
        hyper_loop,
        sl_edge,
        routing_direction,
        routing_slot_positions,
        &mut bend_points,
    );
    add_outer_bend_point(
        graph,
        node,
        hyper_loop,
        sl_edge.target_port,
        routing_slot_positions,
        &mut bend_points,
    );

    bend_points
}

fn add_outer_bend_point(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    port: PortId,
    routing_slot_positions: &[Vec<f64>; 5],
    bend_points: &mut Vec<LPoint>,
) {
    let port_data = &graph.layerless_nodes[node].ports[port];
    let side = port_data.side;
    let slot = hyper_loop.routing_slots[side.ordinal()];
    let mut result = base_vector(side, slot, routing_slot_positions);
    let anchor = LPoint {
        x: port_data.position.x + port_data.anchor.x,
        y: port_data.position.y + port_data.anchor.y,
    };

    match side {
        PortSide::North | PortSide::South => {
            result.x += anchor.x;
        }
        PortSide::East | PortSide::West => {
            result.y += anchor.y;
        }
        PortSide::Undefined => {}
    }

    bend_points.push(result);
}

fn add_corner_bend_points(
    graph: &LGraph,
    node: NodeId,
    hyper_loop: &SelfHyperLoop,
    sl_edge: &SelfLoopEdge,
    routing_direction: EdgeRoutingDirection,
    routing_slot_positions: &[Vec<f64>; 5],
    bend_points: &mut Vec<LPoint>,
) {
    let source_side = graph.layerless_nodes[node].ports[sl_edge.source_port].side;
    let target_side = graph.layerless_nodes[node].ports[sl_edge.target_port].side;

    if source_side == target_side {
        return;
    }

    let mut current_side = source_side;
    while current_side != target_side {
        let next_side = match routing_direction {
            EdgeRoutingDirection::Clockwise => current_side.right(),
            EdgeRoutingDirection::CounterClockwise => current_side.left(),
        };
        let current_component = base_vector(
            current_side,
            hyper_loop.routing_slots[current_side.ordinal()],
            routing_slot_positions,
        );
        let next_component = base_vector(
            next_side,
            hyper_loop.routing_slots[next_side.ordinal()],
            routing_slot_positions,
        );
        bend_points.push(LPoint {
            x: current_component.x + next_component.x,
            y: current_component.y + next_component.y,
        });
        current_side = next_side;
    }
}

fn base_vector(side: PortSide, slot: usize, routing_slot_positions: &[Vec<f64>; 5]) -> LPoint {
    let position = routing_slot_positions[side.ordinal()]
        .get(slot)
        .copied()
        .unwrap_or(0.0);
    match side {
        PortSide::North | PortSide::South => LPoint {
            x: 0.0,
            y: position,
        },
        PortSide::East | PortSide::West => LPoint {
            x: position,
            y: 0.0,
        },
        PortSide::Undefined => LPoint::default(),
    }
}

fn update_node_margins_for_bend_point(
    graph: &LGraph,
    node: NodeId,
    margins: &mut LMargin,
    point: LPoint,
) {
    let size = graph.layerless_nodes[node].size;
    margins.left = margins.left.max(-point.x);
    margins.right = margins.right.max(point.x - size.width);
    margins.top = margins.top.max(-point.y);
    margins.bottom = margins.bottom.max(point.y - size.height);
}

fn place_labels_on_routing_slot(
    graph: &LGraph,
    hyper_loop: &SelfHyperLoop,
    labels: &mut SelfHyperLoopLabels,
    routing_slot_positions: &[Vec<f64>; 5],
) {
    let Some(side) = labels.side else {
        return;
    };
    let slot = hyper_loop.routing_slots[side.ordinal()];
    let Some(mut label_position) = routing_slot_positions[side.ordinal()].get(slot).copied() else {
        return;
    };

    match side {
        PortSide::North => {
            label_position -= graph.options.spacing.edge_label + labels.size.height;
            labels.position.y = label_position;
        }
        PortSide::South => {
            label_position += graph.options.spacing.edge_label;
            labels.position.y = label_position;
        }
        PortSide::West => {
            label_position -= graph.options.spacing.edge_label + labels.size.width;
            labels.position.x = label_position;
        }
        PortSide::East => {
            label_position += graph.options.spacing.edge_label;
            labels.position.x = label_position;
        }
        PortSide::Undefined => {}
    }
}

fn update_node_margins_for_label(
    graph: &LGraph,
    node: NodeId,
    margins: &mut LMargin,
    labels: &SelfHyperLoopLabels,
) {
    update_node_margins_for_bend_point(graph, node, margins, labels.position);
    update_node_margins_for_bend_point(
        graph,
        node,
        margins,
        LPoint {
            x: labels.position.x + labels.size.width,
            y: labels.position.y + labels.size.height,
        },
    );
}
