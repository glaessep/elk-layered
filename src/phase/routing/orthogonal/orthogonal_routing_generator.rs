//! Port of ELK's `OrthogonalRoutingGenerator`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/OrthogonalRoutingGenerator.java

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{LGraph, NodeId, PortRef, PortSide};
use crate::phase::routing::orthogonal::TOLERANCE;
use crate::phase::routing::orthogonal::direction::base_routing_direction_strategy::calculate_bend_points;
use crate::phase::routing::orthogonal::direction::routing_direction::RoutingDirection;
use crate::phase::routing::orthogonal::hyper_edge_cycle_detector::detect_cycles;
use crate::phase::routing::orthogonal::hyper_edge_segment::{
    HyperEdgeSegment, add_port_positions, sort_dedup_f64,
};
use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::HyperEdgeGraph;
use crate::phase::routing::orthogonal::hyper_edge_segment_splitter::split_segments;
use crate::random::JavaRandom;

pub(crate) const CRITICAL_DEPENDENCY_WEIGHT: i32 = 1;

pub(crate) const CRITICAL_CONFLICTS_DETECTED: i32 = -1;

const CONFLICT_THRESHOLD_FACTOR: f64 = 0.5;

const CRITICAL_CONFLICT_THRESHOLD_FACTOR: f64 = 0.2;

pub(crate) const CONFLICT_PENALTY: i32 = 1;

pub(crate) const CROSSING_PENALTY: i32 = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrthogonalRoutingThresholds {
    pub conflict_threshold: f64,
    pub critical_conflict_threshold: f64,
}

impl OrthogonalRoutingThresholds {
    pub fn from_edge_spacing_and_segments(
        edge_spacing: f64,
        segments: &[HyperEdgeSegment],
    ) -> Self {
        Self {
            conflict_threshold: CONFLICT_THRESHOLD_FACTOR * edge_spacing,
            critical_conflict_threshold: CRITICAL_CONFLICT_THRESHOLD_FACTOR
                * minimum_horizontal_segment_distance(segments),
        }
    }
}

pub fn route_edges_west_to_east(
    graph: &mut LGraph,
    source_layer_nodes: Option<&[NodeId]>,
    target_layer_nodes: Option<&[NodeId]>,
    start_pos: f64,
    edge_spacing: f64,
) -> usize {
    route_edges(
        graph,
        RoutingDirection::WestToEast,
        source_layer_nodes,
        target_layer_nodes,
        start_pos,
        edge_spacing,
    )
}

pub fn route_edges(
    graph: &mut LGraph,
    direction: RoutingDirection,
    source_layer_nodes: Option<&[NodeId]>,
    target_layer_nodes: Option<&[NodeId]>,
    start_pos: f64,
    edge_spacing: f64,
) -> usize {
    let mut hyper_graph = HyperEdgeGraph::default();
    let mut port_to_segment = HashMap::new();

    create_hyperedge_segments(
        graph,
        direction,
        source_layer_nodes,
        direction.source_port_side(),
        &mut hyper_graph,
        &mut port_to_segment,
    );
    create_hyperedge_segments(
        graph,
        direction,
        target_layer_nodes,
        direction.target_port_side(),
        &mut hyper_graph,
        &mut port_to_segment,
    );

    let thresholds = OrthogonalRoutingThresholds::from_edge_spacing_and_segments(
        edge_spacing,
        &hyper_graph.segments,
    );
    let mut critical_dependency_count = 0usize;

    for first in 0..hyper_graph.segments.len().saturating_sub(1) {
        for second in (first + 1)..hyper_graph.segments.len() {
            critical_dependency_count +=
                hyper_graph.create_dependency_if_necessary(first, second, thresholds);
        }
    }

    if critical_dependency_count >= 2 {
        break_critical_cycles(&mut hyper_graph, thresholds, &mut graph.random);
    }

    break_non_critical_cycles(&mut hyper_graph, &mut graph.random);
    topological_numbering(&mut hyper_graph);

    // `BaseRoutingDirectionStrategy.createdJunctionPoints`, which `route` clears when it is done.
    let mut created_junction_points = HashSet::new();
    let mut rank_count = -1;
    for segment in 0..hyper_graph.segments.len() {
        if (hyper_graph.segments[segment].start_coordinate()
            - hyper_graph.segments[segment].end_coordinate())
        .abs()
            < TOLERANCE
        {
            continue;
        }

        rank_count = rank_count.max(hyper_graph.segments[segment].routing_slot);
        calculate_bend_points(
            graph,
            direction,
            &hyper_graph,
            segment,
            start_pos,
            edge_spacing,
            &mut created_junction_points,
        );
    }

    (rank_count + 1) as usize
}

pub fn count_crossings(positions: &[f64], start: f64, end: f64) -> i32 {
    let mut crossings = 0;
    for position in positions {
        if *position > end {
            break;
        } else if *position >= start {
            crossings += 1;
        }
    }
    crossings
}

pub fn minimum_horizontal_segment_distance(segments: &[HyperEdgeSegment]) -> f64 {
    let incoming = minimum_difference(
        segments
            .iter()
            .flat_map(|segment| segment.incoming_connection_coordinates.iter().copied()),
    );
    let outgoing = minimum_difference(
        segments
            .iter()
            .flat_map(|segment| segment.outgoing_connection_coordinates.iter().copied()),
    );
    incoming.min(outgoing)
}

fn create_hyperedge_segments(
    graph: &LGraph,
    direction: RoutingDirection,
    nodes: Option<&[NodeId]>,
    port_side: PortSide,
    hyper_graph: &mut HyperEdgeGraph,
    port_to_segment: &mut HashMap<PortRef, usize>,
) {
    let Some(nodes) = nodes else {
        return;
    };

    for node in nodes {
        let Some(lnode) = graph.layerless_nodes.get(*node) else {
            continue;
        };
        for (port_index, port) in lnode.ports.iter_with_ids() {
            let port_ref = PortRef {
                node: *node,
                port: port_index,
            };
            if !graph.port_has_output_type(port_ref) || port.side != port_side {
                continue;
            }

            if !port_to_segment.contains_key(&port_ref) {
                let segment = hyper_graph.add_segment(HyperEdgeSegment::new());
                add_port_positions(
                    graph,
                    direction,
                    port_ref,
                    hyper_graph,
                    segment,
                    port_to_segment,
                );
            }
        }
    }
}

pub fn break_non_critical_cycles(graph: &mut HyperEdgeGraph, random: &mut JavaRandom) {
    let cycle_dependencies = detect_cycles(graph, false, random);

    for dependency in cycle_dependencies {
        if graph.dependencies[dependency].weight == 0 {
            graph.remove_dependency(dependency);
        } else {
            graph.reverse_dependency(dependency);
        }
    }
}

pub fn break_critical_cycles(
    graph: &mut HyperEdgeGraph,
    thresholds: OrthogonalRoutingThresholds,
    random: &mut JavaRandom,
) {
    let cycle_dependencies = detect_cycles(graph, true, random);
    split_segments(graph, &cycle_dependencies, thresholds);
}

pub fn topological_numbering(graph: &mut HyperEdgeGraph) {
    let mut sources = VecDeque::new();
    let mut rightward_targets = VecDeque::new();

    for node in 0..graph.segments.len() {
        graph.segments[node].in_dep_weight = graph.segments[node]
            .incoming_segment_dependencies
            .iter()
            .filter(|dependency| graph.dependencies[**dependency].source.is_some())
            .count() as i32;
        graph.segments[node].out_dep_weight = graph.segments[node]
            .outgoing_segment_dependencies
            .iter()
            .filter(|dependency| graph.dependencies[**dependency].target.is_some())
            .count() as i32;

        if graph.segments[node].in_dep_weight == 0 {
            sources.push_back(node);
        }

        if graph.segments[node].out_dep_weight == 0
            && graph.segments[node]
                .incoming_connection_coordinates
                .is_empty()
        {
            rightward_targets.push_back(node);
        }
    }

    let mut max_rank = -1;

    while let Some(node) = sources.pop_front() {
        for dependency in graph.segments[node].outgoing_segment_dependencies.clone() {
            let Some(target) = graph.dependencies[dependency].target else {
                continue;
            };
            graph.segments[target].routing_slot = graph.segments[target]
                .routing_slot
                .max(graph.segments[node].routing_slot + 1);
            max_rank = max_rank.max(graph.segments[target].routing_slot);

            graph.segments[target].in_dep_weight -= 1;
            if graph.segments[target].in_dep_weight == 0 {
                sources.push_back(target);
            }
        }
    }

    if max_rank > -1 {
        for node in &rightward_targets {
            graph.segments[*node].routing_slot = max_rank;
        }

        while let Some(node) = rightward_targets.pop_front() {
            for dependency in graph.segments[node].incoming_segment_dependencies.clone() {
                let Some(source) = graph.dependencies[dependency].source else {
                    continue;
                };
                if !graph.segments[source]
                    .incoming_connection_coordinates
                    .is_empty()
                {
                    continue;
                }

                graph.segments[source].routing_slot = graph.segments[source]
                    .routing_slot
                    .min(graph.segments[node].routing_slot - 1);
                graph.segments[source].out_dep_weight -= 1;
                if graph.segments[source].out_dep_weight == 0 {
                    rightward_targets.push_back(source);
                }
            }
        }
    }
}

pub(crate) fn count_conflicts(
    positions1: &[f64],
    positions2: &[f64],
    thresholds: OrthogonalRoutingThresholds,
) -> i32 {
    let mut conflicts = 0;

    if !positions1.is_empty() && !positions2.is_empty() {
        let mut index1 = 0usize;
        let mut index2 = 0usize;
        let mut pos1 = positions1[index1];
        let mut pos2 = positions2[index2];
        let mut has_more = true;

        while has_more {
            if pos1 > pos2 - thresholds.critical_conflict_threshold
                && pos1 < pos2 + thresholds.critical_conflict_threshold
            {
                return CRITICAL_CONFLICTS_DETECTED;
            } else if pos1 > pos2 - thresholds.conflict_threshold
                && pos1 < pos2 + thresholds.conflict_threshold
            {
                conflicts += 1;
            }

            if pos1 <= pos2 && index1 + 1 < positions1.len() {
                index1 += 1;
                pos1 = positions1[index1];
            } else if pos2 <= pos1 && index2 + 1 < positions2.len() {
                index2 += 1;
                pos2 = positions2[index2];
            } else {
                has_more = false;
            }
        }
    }

    conflicts
}

fn minimum_difference(numbers: impl Iterator<Item = f64>) -> f64 {
    let mut numbers = numbers.collect::<Vec<_>>();
    sort_dedup_f64(&mut numbers);

    let mut min_difference = f64::MAX;
    if numbers.len() >= 2 {
        let mut current = numbers[0];
        for next in numbers.into_iter().skip(1) {
            min_difference = min_difference.min(next - current);
            current = next;
        }
    }
    min_difference
}

// `OrthogonalRoutingGenerator.createDependencyIfNecessary`, as a method of the graph that owns
// the segments and dependencies.
impl HyperEdgeGraph {
    pub fn create_dependency_if_necessary(
        &mut self,
        first: usize,
        second: usize,
        thresholds: OrthogonalRoutingThresholds,
    ) -> usize {
        let he1 = &self.segments[first];
        let he2 = &self.segments[second];
        if (he1.start_coordinate() - he1.end_coordinate()).abs() < TOLERANCE
            || (he2.start_coordinate() - he2.end_coordinate()).abs() < TOLERANCE
        {
            return 0;
        }

        let conflicts1 = count_conflicts(
            &he1.outgoing_connection_coordinates,
            &he2.incoming_connection_coordinates,
            thresholds,
        );
        let conflicts2 = count_conflicts(
            &he2.outgoing_connection_coordinates,
            &he1.incoming_connection_coordinates,
            thresholds,
        );
        let critical_conflicts_detected =
            conflicts1 == CRITICAL_CONFLICTS_DETECTED || conflicts2 == CRITICAL_CONFLICTS_DETECTED;
        let mut critical_dependency_count = 0usize;

        if critical_conflicts_detected {
            if conflicts1 == CRITICAL_CONFLICTS_DETECTED {
                self.add_critical_dependency(second, first);
                critical_dependency_count += 1;
            }

            if conflicts2 == CRITICAL_CONFLICTS_DETECTED {
                self.add_critical_dependency(first, second);
                critical_dependency_count += 1;
            }
        } else {
            let crossings1 = count_crossings(
                &he1.outgoing_connection_coordinates,
                he2.start_coordinate(),
                he2.end_coordinate(),
            ) + count_crossings(
                &he2.incoming_connection_coordinates,
                he1.start_coordinate(),
                he1.end_coordinate(),
            );
            let crossings2 = count_crossings(
                &he2.outgoing_connection_coordinates,
                he1.start_coordinate(),
                he1.end_coordinate(),
            ) + count_crossings(
                &he1.incoming_connection_coordinates,
                he2.start_coordinate(),
                he2.end_coordinate(),
            );

            let dep_value1 = CONFLICT_PENALTY * conflicts1 + CROSSING_PENALTY * crossings1;
            let dep_value2 = CONFLICT_PENALTY * conflicts2 + CROSSING_PENALTY * crossings2;

            if dep_value1 < dep_value2 {
                self.add_regular_dependency(first, second, dep_value2 - dep_value1);
            } else if dep_value1 > dep_value2 {
                self.add_regular_dependency(second, first, dep_value1 - dep_value2);
            } else if dep_value1 > 0 && dep_value2 > 0 {
                self.add_regular_dependency(first, second, 0);
                self.add_regular_dependency(second, first, 0);
            }
        }

        critical_dependency_count
    }
}
