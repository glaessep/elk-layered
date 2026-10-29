//! Tests of the classes in this module.

use std::collections::HashSet;

use crate::graph::{EdgeId, LPoint, PortSide};
use crate::graph::{LGraph, LNode, LayeredEdge, PortType};
use crate::options::LayeredOptions;
use crate::phase::routing::orthogonal::direction::base_routing_direction_strategy::add_junction_point_if_necessary;
use crate::phase::routing::orthogonal::direction::routing_direction::RoutingDirection;
use crate::phase::routing::orthogonal::hyper_edge_cycle_detector::detect_cycles;
use crate::phase::routing::orthogonal::hyper_edge_segment::HyperEdgeSegment;
use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::{
    DependencyType, HyperEdgeGraph,
};
use crate::phase::routing::orthogonal::hyper_edge_segment_splitter::split_segments;
use crate::phase::routing::orthogonal::hyper_edge_segment_splitter::{
    AreaRating, FreeArea, rate_area,
};
use crate::phase::routing::orthogonal::orthogonal_routing_generator::{
    OrthogonalRoutingThresholds, break_critical_cycles, break_non_critical_cycles, route_edges,
    route_edges_west_to_east, topological_numbering,
};
use crate::random::JavaRandom;

/// A graph with one edge, to hold junction points.
fn graph_with_one_edge() -> LGraph {
    let graph = route_test_graph(0.0, 50.0);
    assert_eq!(graph.edges.len(), 1);
    graph
}

fn junction(
    segment: &HyperEdgeSegment,
    position: LPoint,
    vertical: bool,
    created: &mut HashSet<(u64, u64)>,
    graph: &mut LGraph,
) {
    add_junction_point_if_necessary(graph, EdgeId::new(0), segment, position, vertical, created);
}

// Mutation: in `rate_area`, call `split_segment.recompute_extent()` after the area centre
// joins its connections. A second mutation calls `split_partner.recompute_extent()` there.
// Either one counts 3 crossings instead of 2.
#[test]
fn rating_an_area_keeps_the_extent_of_the_simulated_split() {
    // The segment to split runs from 0 to 10 in and from 40 to 50 out. The segment that causes
    // the split runs from 20 to 30, and the free area has its centre at 25.
    let mut segment = HyperEdgeSegment::with_incoming_outgoing([0.0, 10.0], [40.0, 50.0]);
    segment.split_by = Some(1);
    let split_by = HyperEdgeSegment::with_incoming_outgoing([20.0], [30.0]);
    let mut graph = HyperEdgeGraph::default();
    graph.add_segment(segment.clone());
    graph.add_segment(split_by);
    let (mut split_segment, mut split_partner) = segment.simulate_split();

    let rating = rate_area(
        &graph,
        &segment,
        &mut split_segment,
        &mut split_partner,
        FreeArea::new(20.0, 30.0),
    );

    // The centre 25 crosses the causing segment once on each side. The split segment keeps
    // its extent 0 to 10, so the causing segment's incoming 20 misses it, and the partner
    // keeps 40 to 50, so the outgoing 30 misses it too.
    assert_eq!(
        rating,
        AreaRating {
            dependencies: 2,
            crossings: 2,
        }
    );
    assert_eq!(
        (
            split_segment.start_coordinate(),
            split_segment.end_coordinate()
        ),
        (0.0, 10.0)
    );
    assert_eq!(
        (
            split_partner.start_coordinate(),
            split_partner.end_coordinate()
        ),
        (40.0, 50.0)
    );
}

// Mutation: use the `x` coordinate for a vertical connecting segment, or drop the strict
// inequalities. A point at an end of the segment is then a junction.
#[test]
fn a_junction_point_is_needed_only_inside_the_segment_or_where_two_edges_join_at_an_end() {
    // The segment runs from 10 to 40 on the connecting axis. Edges come in at 10 and 40, and
    // leave at 10 and 40, so both ends have a joining edge on each side.
    let both_ends = segment(&[10.0, 40.0], &[10.0, 40.0]);
    // Inside the segment: a junction. Vertical means the connecting segment is vertical, so
    // the axis is `y`.
    let mut graph = graph_with_one_edge();
    let mut created = HashSet::new();
    junction(
        &both_ends,
        LPoint { x: 99.0, y: 25.0 },
        true,
        &mut created,
        &mut graph,
    );
    junction(
        &both_ends,
        LPoint { x: 25.0, y: 99.0 },
        false,
        &mut created,
        &mut graph,
    );
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 2);

    // At the ends, with edges on both sides at that coordinate: a junction.
    let mut graph = graph_with_one_edge();
    junction(
        &both_ends,
        LPoint { x: 0.0, y: 10.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    junction(
        &both_ends,
        LPoint { x: 0.0, y: 40.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 2);

    // At the ends of a segment whose incoming and outgoing edges end elsewhere: none.
    let apart = segment(&[10.0, 30.0], &[20.0, 40.0]);
    let mut graph = graph_with_one_edge();
    junction(
        &apart,
        LPoint { x: 0.0, y: 10.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    junction(
        &apart,
        LPoint { x: 0.0, y: 40.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    assert!(graph.edges[EdgeId::new(0)].junction_points.is_empty());

    // Outside the segment, and on a segment without outgoing edges: none.
    let open = segment(&[10.0, 40.0], &[]);
    junction(
        &open,
        LPoint { x: 0.0, y: 5.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    junction(
        &open,
        LPoint { x: 0.0, y: 10.0 },
        true,
        &mut HashSet::new(),
        &mut graph,
    );
    assert!(graph.edges[EdgeId::new(0)].junction_points.is_empty());
}

// Mutation: do not remember the junction points that were made. A bend point that two
// edges share then gets its junction point twice.
#[test]
fn a_junction_point_is_made_once_for_a_position() {
    let both_ends = segment(&[10.0, 40.0], &[10.0, 40.0]);
    let mut graph = graph_with_one_edge();
    let mut created = HashSet::new();
    for _ in 0..3 {
        junction(
            &both_ends,
            LPoint { x: 7.0, y: 25.0 },
            true,
            &mut created,
            &mut graph,
        );
    }
    assert_eq!(
        graph.edges[EdgeId::new(0)].junction_points,
        [LPoint { x: 7.0, y: 25.0 }]
    );
    junction(
        &both_ends,
        LPoint { x: 8.0, y: 25.0 },
        true,
        &mut created,
        &mut graph,
    );
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 2);
}

// Mutation: tell `-0.0` from `0.0` by the bits of the point, as a JVM hash does. The second
// point is then a new junction point.
#[test]
fn a_negative_zero_is_the_same_junction_point_as_a_zero() {
    let both_ends = segment(&[10.0, 40.0], &[10.0, 40.0]);
    let mut graph = graph_with_one_edge();
    let mut created = HashSet::new();
    for x in [0.0, -0.0] {
        junction(
            &both_ends,
            LPoint { x, y: 25.0 },
            true,
            &mut created,
            &mut graph,
        );
    }
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 1);
    // The same holds for the other coordinate, and when the negative zero comes first.
    let mut graph = graph_with_one_edge();
    let mut created = HashSet::new();
    for y in [-0.0, 0.0] {
        junction(
            &segment(&[-10.0, 10.0], &[-10.0, 10.0]),
            LPoint { x: 7.0, y },
            true,
            &mut created,
            &mut graph,
        );
    }
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 1);
}

// Mutation: remember a point with a `NaN` by its bits. The second one is then taken for a
// point seen before, and only one junction point is made.
#[test]
fn a_point_with_a_nan_is_never_seen_before() {
    let both_ends = segment(&[10.0, 40.0], &[10.0, 40.0]);
    let mut graph = graph_with_one_edge();
    let mut created = HashSet::new();
    for _ in 0..2 {
        junction(
            &both_ends,
            LPoint {
                x: f64::NAN,
                y: 25.0,
            },
            true,
            &mut created,
            &mut graph,
        );
    }
    assert_eq!(graph.edges[EdgeId::new(0)].junction_points.len(), 2);
    assert!(created.is_empty());
}

fn segment(incoming: &[f64], outgoing: &[f64]) -> HyperEdgeSegment {
    HyperEdgeSegment::with_incoming_outgoing(incoming.to_vec(), outgoing.to_vec())
}

fn route_test_graph(source_y: f64, target_y: f64) -> LGraph {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let source = graph.next_node_id();
    let mut source_node = LNode::new("A", 40.0, 20.0, Some(0));
    source_node.position = LPoint {
        x: 0.0,
        y: source_y,
    };
    graph.layerless_nodes.push(source_node);

    let target = graph.next_node_id();
    let mut target_node = LNode::new("B", 40.0, 20.0, Some(1));
    target_node.position = LPoint {
        x: 100.0,
        y: target_y,
    };
    graph.layerless_nodes.push(target_node);

    graph.set_node_layer(source, 0);
    graph.set_node_layer(target, 1);

    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::East,
            LPoint { x: 40.0, y: 5.0 },
        )
        .unwrap();
    let target_port = graph
        .add_port(
            target,
            PortType::Output,
            PortSide::West,
            LPoint { x: 0.0, y: 15.0 },
        )
        .unwrap();

    graph.add_edge({
        let mut built_edge = LayeredEdge::new("A-B".to_string(), source_port, target_port);
        built_edge.source_node_id = "A".to_string();
        built_edge.target_node_id = "B".to_string();
        built_edge.model_order = Some(0);
        built_edge
    });

    graph
}

fn vertical_route_test_graph(source_x: f64, target_x: f64, source_side: PortSide) -> LGraph {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let source = graph.next_node_id();
    let mut source_node = LNode::new("A", 20.0, 40.0, Some(0));
    source_node.position = LPoint {
        x: source_x,
        y: 0.0,
    };
    graph.layerless_nodes.push(source_node);

    let target = graph.next_node_id();
    let mut target_node = LNode::new("B", 20.0, 40.0, Some(1));
    target_node.position = LPoint {
        x: target_x,
        y: 100.0,
    };
    graph.layerless_nodes.push(target_node);

    graph.set_node_layer(source, 0);
    graph.set_node_layer(target, 1);

    let source_port_side = source_side;
    let target_port_side = source_side.opposed();
    let source_port_position = match source_port_side {
        PortSide::North => LPoint { x: 5.0, y: 0.0 },
        PortSide::South => LPoint { x: 5.0, y: 40.0 },
        _ => unreachable!("vertical test only uses north/south source ports"),
    };
    let target_port_position = match target_port_side {
        PortSide::North => LPoint { x: 15.0, y: 0.0 },
        PortSide::South => LPoint { x: 15.0, y: 40.0 },
        _ => unreachable!("vertical test only uses north/south target ports"),
    };

    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            source_port_side,
            source_port_position,
        )
        .unwrap();
    let target_port = graph
        .add_port(
            target,
            PortType::Output,
            target_port_side,
            target_port_position,
        )
        .unwrap();

    graph.add_edge({
        let mut built_edge = LayeredEdge::new("A-B".to_string(), source_port, target_port);
        built_edge.source_node_id = "A".to_string();
        built_edge.target_node_id = "B".to_string();
        built_edge.model_order = Some(0);
        built_edge
    });

    graph
}

fn has_active_dependency(
    graph: &HyperEdgeGraph,
    source: usize,
    target: usize,
    dependency_type: DependencyType,
) -> bool {
    graph.dependencies.iter().any(|dependency| {
        dependency.source == Some(source)
            && dependency.target == Some(target)
            && dependency.dependency_type == dependency_type
    })
}

#[test]
fn segment_extent_tracks_sorted_unique_connection_coordinates() {
    let mut segment = HyperEdgeSegment::new();

    segment.insert_incoming(30.0);
    segment.insert_incoming(10.0);
    segment.insert_incoming(10.0);
    segment.insert_outgoing(20.0);

    assert_eq!(segment.incoming_connection_coordinates, vec![10.0, 30.0]);
    assert_eq!(segment.outgoing_connection_coordinates, vec![20.0]);
    assert_eq!(segment.start_coordinate(), 10.0);
    assert_eq!(segment.end_coordinate(), 30.0);
    assert_eq!(segment.length(), 20.0);
}

#[test]
fn dependency_reverse_and_remove_keep_segment_adjacency_lists_in_sync() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0], &[30.0]));
    let b = graph.add_segment(segment(&[10.0], &[40.0]));
    let dependency = graph.add_regular_dependency(a, b, 7);

    assert_eq!(
        graph.segments[a].outgoing_segment_dependencies,
        vec![dependency]
    );
    assert_eq!(
        graph.segments[b].incoming_segment_dependencies,
        vec![dependency]
    );

    graph.reverse_dependency(dependency);

    assert_eq!(graph.dependencies[dependency].source, Some(b));
    assert_eq!(graph.dependencies[dependency].target, Some(a));
    assert!(graph.segments[a].outgoing_segment_dependencies.is_empty());
    assert_eq!(
        graph.segments[a].incoming_segment_dependencies,
        vec![dependency]
    );
    assert_eq!(
        graph.segments[b].outgoing_segment_dependencies,
        vec![dependency]
    );
    assert!(graph.segments[b].incoming_segment_dependencies.is_empty());

    graph.remove_dependency(dependency);

    assert!(graph.dependencies[dependency].source.is_none());
    assert!(graph.dependencies[dependency].target.is_none());
    assert!(graph.segments[a].incoming_segment_dependencies.is_empty());
    assert!(graph.segments[b].outgoing_segment_dependencies.is_empty());
}

#[test]
fn create_dependency_if_necessary_adds_critical_dependency_for_overlaps() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0], &[20.0]));
    let b = graph.add_segment(segment(&[20.4], &[40.0]));
    let thresholds = OrthogonalRoutingThresholds {
        conflict_threshold: 5.0,
        critical_conflict_threshold: 1.0,
    };

    let critical_count = graph.create_dependency_if_necessary(a, b, thresholds);

    assert_eq!(critical_count, 1);
    assert_eq!(graph.dependencies.len(), 1);
    assert_eq!(
        graph.dependencies[0].dependency_type,
        DependencyType::Critical
    );
    assert_eq!(graph.dependencies[0].source, Some(b));
    assert_eq!(graph.dependencies[0].target, Some(a));
}

#[test]
fn break_non_critical_cycles_reverses_positive_weight_back_edges() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0], &[30.0]));
    let b = graph.add_segment(segment(&[10.0], &[40.0]));
    let c = graph.add_segment(segment(&[20.0], &[50.0]));
    graph.add_regular_dependency(a, b, 2);
    graph.add_regular_dependency(b, c, 2);
    let back_edge = graph.add_regular_dependency(c, a, 1);
    let mut random = JavaRandom::new(1);

    break_non_critical_cycles(&mut graph, &mut random);

    assert_eq!(graph.dependencies[back_edge].source, Some(a));
    assert_eq!(graph.dependencies[back_edge].target, Some(c));
}

#[test]
fn topological_numbering_assigns_increasing_slots() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0], &[30.0]));
    let b = graph.add_segment(segment(&[10.0], &[40.0]));
    let c = graph.add_segment(segment(&[20.0], &[50.0]));
    graph.add_regular_dependency(a, b, 1);
    graph.add_regular_dependency(b, c, 1);

    topological_numbering(&mut graph);

    assert_eq!(graph.segments[a].routing_slot, 0);
    assert_eq!(graph.segments[b].routing_slot, 1);
    assert_eq!(graph.segments[c].routing_slot, 2);
}

#[test]
fn split_segment_at_moves_outgoing_coordinates_and_clears_dependencies() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0, 10.0], &[30.0, 40.0]));
    let b = graph.add_segment(segment(&[20.0], &[50.0]));
    graph.add_regular_dependency(a, b, 1);

    let partner = graph.split_segment_at(a, 25.0);

    assert_eq!(
        graph.segments[a].outgoing_connection_coordinates,
        vec![25.0]
    );
    assert_eq!(
        graph.segments[partner].incoming_connection_coordinates,
        vec![25.0]
    );
    assert_eq!(
        graph.segments[partner].outgoing_connection_coordinates,
        vec![30.0, 40.0]
    );
    assert_eq!(graph.segments[a].split_partner, Some(partner));
    assert_eq!(graph.segments[partner].split_partner, Some(a));
    assert!(graph.segments[a].outgoing_segment_dependencies.is_empty());
    assert!(graph.segments[b].incoming_segment_dependencies.is_empty());
}

#[test]
fn split_segments_uses_reachable_free_area_and_rebuilds_critical_chain() {
    let mut graph = HyperEdgeGraph::default();
    let obstacle = graph.add_segment(segment(&[0.0], &[20.0]));
    let split = graph.add_segment(segment(&[20.4], &[40.0]));
    let dependency = graph.add_critical_dependency(split, obstacle);
    let thresholds = OrthogonalRoutingThresholds {
        conflict_threshold: 5.0,
        critical_conflict_threshold: 1.0,
    };

    split_segments(&mut graph, &[dependency], thresholds);

    let partner = graph.segments[split].split_partner.unwrap();
    assert_eq!(graph.segments[split].split_by, Some(obstacle));
    assert_eq!(
        graph.segments[split].outgoing_connection_coordinates,
        vec![30.2]
    );
    assert_eq!(
        graph.segments[partner].incoming_connection_coordinates,
        vec![30.2]
    );
    assert_eq!(
        graph.segments[partner].outgoing_connection_coordinates,
        vec![40.0]
    );
    assert!(has_active_dependency(
        &graph,
        split,
        obstacle,
        DependencyType::Critical
    ));
    assert!(has_active_dependency(
        &graph,
        obstacle,
        partner,
        DependencyType::Critical
    ));
}

#[test]
fn break_critical_cycles_splits_segment_and_removes_critical_cycle() {
    let mut graph = HyperEdgeGraph::default();
    let a = graph.add_segment(segment(&[0.0], &[20.0]));
    let b = graph.add_segment(segment(&[20.4], &[0.4]));
    graph.add_critical_dependency(a, b);
    graph.add_critical_dependency(b, a);
    let thresholds = OrthogonalRoutingThresholds {
        conflict_threshold: 5.0,
        critical_conflict_threshold: 1.0,
    };
    let mut random = JavaRandom::new(1);

    break_critical_cycles(&mut graph, thresholds, &mut random);

    assert_eq!(graph.segments.len(), 3);
    assert!(
        graph
            .segments
            .iter()
            .any(|segment| segment.split_by.is_some())
    );
    assert!(detect_cycles(&mut graph, true, &mut JavaRandom::new(1)).is_empty());
}

#[test]
fn symmetric_critical_cycle_uses_elk_mark_order_for_seeded_split_choice() {
    // Java Random selects index 1 for seed 1 and index 0 for seed 4096.
    // ELK's mark-ordered candidates are [b, a]; the later-marked segment
    // owns the backward dependency and must be split around the other one.
    for (seed, expected_split) in [(1, 1), (4096, 0)] {
        let mut graph = HyperEdgeGraph::default();
        let a = graph.add_segment(segment(&[0.0], &[20.0]));
        let b = graph.add_segment(segment(&[20.0], &[0.0]));
        graph.add_critical_dependency(a, b);
        graph.add_critical_dependency(b, a);
        let thresholds = OrthogonalRoutingThresholds {
            conflict_threshold: 5.0,
            critical_conflict_threshold: 1.0,
        };

        break_critical_cycles(&mut graph, thresholds, &mut JavaRandom::new(seed));

        let obstacle = 1 - expected_split;
        assert_eq!(graph.segments.len(), 3, "seed={seed}");
        assert_eq!(
            graph.segments[expected_split].split_by,
            Some(obstacle),
            "seed={seed}"
        );
        assert_eq!(graph.segments[obstacle].split_by, None, "seed={seed}");
        let partner = graph.segments[expected_split].split_partner.unwrap();
        assert!(has_active_dependency(
            &graph,
            expected_split,
            obstacle,
            DependencyType::Critical
        ));
        assert!(has_active_dependency(
            &graph,
            obstacle,
            partner,
            DependencyType::Critical
        ));
        assert!(detect_cycles(&mut graph, true, &mut JavaRandom::new(seed)).is_empty());
    }
}

#[test]
fn route_edges_west_to_east_writes_orthogonal_bendpoints_for_non_straight_edge() {
    let mut graph = route_test_graph(0.0, 30.0);
    let left = graph.layers()[0].nodes().to_vec();
    let right = graph.layers()[1].nodes().to_vec();

    let slots = route_edges_west_to_east(&mut graph, Some(&left), Some(&right), 50.0, 10.0);

    assert_eq!(slots, 1);
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 50.0, y: 5.0 }, LPoint { x: 50.0, y: 45.0 }]
    );
}

#[test]
fn route_edges_west_to_east_uses_actual_outgoing_edges_for_output_ports() {
    let mut graph = route_test_graph(0.0, 30.0);
    let source_port = graph.edges[EdgeId::new(0)].source();
    graph.layerless_nodes[source_port.node].ports[source_port.port].port_type = PortType::Input;
    let left = graph.layers()[0].nodes().to_vec();
    let right = graph.layers()[1].nodes().to_vec();

    let slots = route_edges_west_to_east(&mut graph, Some(&left), Some(&right), 50.0, 10.0);

    assert_eq!(slots, 1);
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 50.0, y: 5.0 }, LPoint { x: 50.0, y: 45.0 }]
    );
}

#[test]
fn route_edges_west_to_east_leaves_straight_edge_without_bendpoints() {
    let mut graph = route_test_graph(0.0, -10.0);
    let left = graph.layers()[0].nodes().to_vec();
    let right = graph.layers()[1].nodes().to_vec();

    let slots = route_edges_west_to_east(&mut graph, Some(&left), Some(&right), 50.0, 10.0);

    assert_eq!(slots, 0);
    assert!(graph.edges[EdgeId::new(0)].bend_points.is_empty());
}

#[test]
fn route_edges_north_to_south_writes_orthogonal_bendpoints_for_non_straight_edge() {
    let mut graph = vertical_route_test_graph(0.0, 30.0, PortSide::South);
    let source = graph.layers()[0].nodes().to_vec();
    let target = graph.layers()[1].nodes().to_vec();

    let slots = route_edges(
        &mut graph,
        RoutingDirection::NorthToSouth,
        Some(&source),
        Some(&target),
        60.0,
        10.0,
    );

    assert_eq!(slots, 1);
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 5.0, y: 60.0 }, LPoint { x: 45.0, y: 60.0 }]
    );
}

#[test]
fn route_edges_south_to_north_writes_orthogonal_bendpoints_for_non_straight_edge() {
    let mut graph = vertical_route_test_graph(0.0, 30.0, PortSide::North);
    let source = graph.layers()[0].nodes().to_vec();
    let target = graph.layers()[1].nodes().to_vec();

    let slots = route_edges(
        &mut graph,
        RoutingDirection::SouthToNorth,
        Some(&source),
        Some(&target),
        -20.0,
        10.0,
    );

    assert_eq!(slots, 1);
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 5.0, y: -20.0 }, LPoint { x: 45.0, y: -20.0 }]
    );
}

// Mutation: let `add_port_positions` recurse into each connected port again.
#[test]
fn a_long_chain_of_connected_ports_routes_on_a_small_stack() {
    // Sources in layer 0 and targets in layer 1. Source i reaches targets i - 1 and i, so the ports
    // form one hyperedge whose walk goes 8,000 ports deep.
    const PAIRS: usize = 4_000;
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let mut sources = Vec::new();
    let mut targets = Vec::new();
    for i in 0..PAIRS {
        for (layer, x, side, list) in [
            (0, 0.0, PortSide::East, &mut sources),
            (1, 100.0, PortSide::West, &mut targets),
        ] {
            let node = graph.next_node_id();
            let mut lnode = LNode::new(format!("{layer}-{i}"), 40.0, 20.0, None);
            lnode.position = LPoint {
                x,
                y: 30.0 * i as f64,
            };
            graph.layerless_nodes.push(lnode);
            graph.set_node_layer(node, layer);
            let port_x = if side == PortSide::East { 40.0 } else { 0.0 };
            let port = graph
                .add_port(node, PortType::Output, side, LPoint { x: port_x, y: 10.0 })
                .unwrap();
            list.push(port);
        }
    }
    for (i, &source) in sources.iter().enumerate() {
        for target in [i.checked_sub(1), Some(i)].into_iter().flatten() {
            graph.add_edge(LayeredEdge::new(
                format!("e{i}-{target}"),
                source,
                targets[target],
            ));
        }
    }

    let routed = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            crate::phase::routing::route_edges_orthogonal(&mut graph);
            graph.edges.len()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(routed, 2 * PAIRS - 1);
}
