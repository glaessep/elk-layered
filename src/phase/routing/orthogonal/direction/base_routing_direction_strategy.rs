//! Port of ELK's `BaseRoutingDirectionStrategy`, with the `calculateBendPoints` that each of its
//! three subclasses has. The three differ only in direction, so one function serves them, and
//! `RoutingDirection` gives what differs.
//!
//! Sources:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/BaseRoutingDirectionStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/direction/WestToEastRoutingStrategy.java

use std::collections::HashSet;

use crate::graph::{EdgeId, LGraph, LPoint, PortRef};
use crate::phase::routing::orthogonal::TOLERANCE;
use crate::phase::routing::orthogonal::direction::routing_direction::RoutingDirection;
use crate::phase::routing::orthogonal::hyper_edge_segment::HyperEdgeSegment;
use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::HyperEdgeGraph;

pub(crate) fn calculate_bend_points(
    graph: &mut LGraph,
    direction: RoutingDirection,
    hyper_graph: &HyperEdgeGraph,
    segment_index: usize,
    start_pos: f64,
    edge_spacing: f64,
    created_junction_points: &mut HashSet<(u64, u64)>,
) {
    let segment = &hyper_graph.segments[segment_index];
    // The three strategies differ only in which axis the connecting segment lies on.
    let vertical = direction == RoutingDirection::WestToEast;
    if segment.is_dummy() {
        return;
    }

    let segment_position =
        direction.routing_slot_position(start_pos, segment.routing_slot, edge_spacing);

    for port_ref in &segment.ports {
        let source_anchor = absolute_anchor(graph, *port_ref);
        let outgoing_edges = graph.layerless_nodes[port_ref.node].ports[port_ref.port]
            .outgoing_edges()
            .to_vec();

        for edge_index in outgoing_edges {
            if is_self_loop(graph, edge_index) {
                continue;
            }

            let target = graph.edges[edge_index].target();
            let target_anchor = absolute_anchor(graph, target);

            if bend_point_axis_delta(direction, source_anchor, target_anchor).abs() <= TOLERANCE {
                continue;
            }

            let mut current_position = segment_position;
            let mut current_segment = segment;

            let bend = source_bend_point(direction, source_anchor, current_position);
            add_bend_point_if_needed(graph, edge_index, bend);
            add_junction_point_if_necessary(
                graph,
                edge_index,
                segment,
                bend,
                vertical,
                created_junction_points,
            );

            if let Some(split_partner) = segment.split_partner {
                let split_position = hyper_graph.segments[split_partner]
                    .incoming_connection_coordinates
                    .first()
                    .copied()
                    .unwrap_or_else(|| direction.port_position_on_hypernode(graph, *port_ref));

                let bend = split_bend_point(direction, split_position, current_position);
                add_bend_point_if_needed(graph, edge_index, bend);
                add_junction_point_if_necessary(
                    graph,
                    edge_index,
                    segment,
                    bend,
                    vertical,
                    created_junction_points,
                );

                current_position = direction.routing_slot_position(
                    start_pos,
                    hyper_graph.segments[split_partner].routing_slot,
                    edge_spacing,
                );

                current_segment = &hyper_graph.segments[split_partner];
                let bend = split_bend_point(direction, split_position, current_position);
                add_bend_point_if_needed(graph, edge_index, bend);
                add_junction_point_if_necessary(
                    graph,
                    edge_index,
                    current_segment,
                    bend,
                    vertical,
                    created_junction_points,
                );
            }

            let bend = target_bend_point(direction, target_anchor, current_position);
            add_bend_point_if_needed(graph, edge_index, bend);
            add_junction_point_if_necessary(
                graph,
                edge_index,
                current_segment,
                bend,
                vertical,
                created_junction_points,
            );
        }
    }
}

pub(crate) fn absolute_anchor(graph: &LGraph, port_ref: PortRef) -> LPoint {
    let node = &graph.layerless_nodes[port_ref.node];
    let port = &node.ports[port_ref.port];
    LPoint {
        x: node.position.x + port.position.x + port.anchor.x,
        y: node.position.y + port.position.y + port.anchor.y,
    }
}

fn bend_point_axis_delta(
    direction: RoutingDirection,
    source_anchor: LPoint,
    target_anchor: LPoint,
) -> f64 {
    match direction {
        RoutingDirection::WestToEast => source_anchor.y - target_anchor.y,
        RoutingDirection::NorthToSouth | RoutingDirection::SouthToNorth => {
            source_anchor.x - target_anchor.x
        }
    }
}

fn source_bend_point(
    direction: RoutingDirection,
    source_anchor: LPoint,
    routing_position: f64,
) -> LPoint {
    match direction {
        RoutingDirection::WestToEast => LPoint {
            x: routing_position,
            y: source_anchor.y,
        },
        RoutingDirection::NorthToSouth | RoutingDirection::SouthToNorth => LPoint {
            x: source_anchor.x,
            y: routing_position,
        },
    }
}

fn split_bend_point(
    direction: RoutingDirection,
    split_position: f64,
    routing_position: f64,
) -> LPoint {
    match direction {
        RoutingDirection::WestToEast => LPoint {
            x: routing_position,
            y: split_position,
        },
        RoutingDirection::NorthToSouth | RoutingDirection::SouthToNorth => LPoint {
            x: split_position,
            y: routing_position,
        },
    }
}

fn target_bend_point(
    direction: RoutingDirection,
    target_anchor: LPoint,
    routing_position: f64,
) -> LPoint {
    match direction {
        RoutingDirection::WestToEast => LPoint {
            x: routing_position,
            y: target_anchor.y,
        },
        RoutingDirection::NorthToSouth | RoutingDirection::SouthToNorth => LPoint {
            x: target_anchor.x,
            y: routing_position,
        },
    }
}

fn is_self_loop(graph: &LGraph, edge_index: EdgeId) -> bool {
    graph.edges[edge_index].source().node == graph.edges[edge_index].target().node
}

/// Ports `BaseRoutingDirectionStrategy.addJunctionPointIfNecessary`: a bend point needs a junction
/// point when it is not at one of the two ends of the hyperedge segment.
pub(crate) fn add_junction_point_if_necessary(
    graph: &mut LGraph,
    edge_index: EdgeId,
    segment: &HyperEdgeSegment,
    position: LPoint,
    vertical: bool,
    created: &mut HashSet<(u64, u64)>,
) {
    let p = if vertical { position.y } else { position.x };

    let key = junction_key(position);
    if key.is_some_and(|key| created.contains(&key)) {
        return;
    }

    let inside_segment = p > segment.start_coordinate() && p < segment.end_coordinate();

    let incoming = &segment.incoming_connection_coordinates;
    let outgoing = &segment.outgoing_connection_coordinates;
    let mut at_boundary = false;
    if let (Some(first_in), Some(last_in), Some(first_out), Some(last_out)) = (
        incoming.first(),
        incoming.last(),
        outgoing.first(),
        outgoing.last(),
    ) {
        at_boundary |= (p - first_in).abs() < TOLERANCE && (p - first_out).abs() < TOLERANCE;
        at_boundary |= (p - last_in).abs() < TOLERANCE && (p - last_out).abs() < TOLERANCE;
    }

    if inside_segment || at_boundary {
        graph.edges[edge_index].junction_points.push(position);
        if let Some(key) = key {
            created.insert(key);
        }
    }
}

/// What `HashSet<KVector>` finds a point by in `createdJunctionPoints`, or `None` for a point it
/// never finds.
///
/// elkjs is GWT, and `Double.hashCode` there is `(int) value`. So `-0.0` and `0.0` fall in one
/// bucket, and `KVector.equals`, which uses `==`, finds them equal: a point with a negative zero
/// is the point with a zero. A JVM hashes the bits and tells them apart. A `NaN` is never equal
/// to itself, so a point with a `NaN` is never found, however often it is added.
fn junction_key(position: LPoint) -> Option<(u64, u64)> {
    if position.x.is_nan() || position.y.is_nan() {
        return None;
    }
    // Adding 0.0 turns -0.0 into 0.0 and changes no other number.
    Some(((position.x + 0.0).to_bits(), (position.y + 0.0).to_bits()))
}

fn add_bend_point_if_needed(graph: &mut LGraph, edge_index: EdgeId, point: LPoint) {
    if let Some(last) = graph.edges[edge_index].bend_points.last()
        && (last.x - point.x).abs() < TOLERANCE
        && (last.y - point.y).abs() < TOLERANCE
    {
        return;
    }

    graph.edges[edge_index].bend_points.push(point);
}
