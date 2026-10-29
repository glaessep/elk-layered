//! Port of ELK's `RoutingSlotAssigner`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/routing/RoutingSlotAssigner.java

use std::collections::VecDeque;

use crate::graph::{LGraph, NodeId, PortSide, SelfHyperLoop, SelfLoopHolder};
use crate::p5edges::orthogonal::{HyperEdgeGraph, HyperEdgeSegment, break_non_critical_cycles};
use crate::selfloop::routing::routing_director::PortPlaces;

pub(crate) fn assign_routing_slots(
    graph: &mut LGraph,
    holder: &mut SelfLoopHolder,
    places: &PortPlaces,
) {
    let label_crossing_matrix = compute_self_loop_label_crossing_matrix(holder);
    let activity_over_ports = compute_loop_activity(holder, places);
    let mut slot_graph = create_self_loop_crossing_graph(
        places,
        holder,
        &activity_over_ports,
        &label_crossing_matrix,
    );

    break_non_critical_cycles(&mut slot_graph, &mut graph.random);
    assign_raw_routing_slots_to_segments(&mut slot_graph);
    assign_raw_routing_slots_to_loops(holder, &slot_graph);
    shift_slots_towards_node(graph, holder, &activity_over_ports, &label_crossing_matrix);
}

fn compute_self_loop_label_crossing_matrix(holder: &mut SelfLoopHolder) -> Vec<Vec<bool>> {
    let mut label_id = 0usize;
    for hyper_loop in &mut holder.hyper_loops {
        if let Some(labels) = &mut hyper_loop.labels {
            labels.id = Some(label_id);
            label_id += 1;
        }
    }

    let mut matrix = vec![vec![false; label_id]; label_id];
    for first in 0..holder.hyper_loops.len().saturating_sub(1) {
        for second in (first + 1)..holder.hyper_loops.len() {
            if self_loop_labels_overlap(&holder.hyper_loops[first], &holder.hyper_loops[second])
                && let (Some(first_id), Some(second_id)) = (
                    holder.hyper_loops[first]
                        .labels
                        .as_ref()
                        .and_then(|labels| labels.id),
                    holder.hyper_loops[second]
                        .labels
                        .as_ref()
                        .and_then(|labels| labels.id),
                )
            {
                matrix[first_id][second_id] = true;
                matrix[second_id][first_id] = true;
            }
        }
    }

    matrix
}

pub(crate) fn self_loop_labels_overlap(first: &SelfHyperLoop, second: &SelfHyperLoop) -> bool {
    let (Some(first_labels), Some(second_labels)) = (&first.labels, &second.labels) else {
        return false;
    };
    let (Some(first_side), Some(second_side)) = (first_labels.side, second_labels.side) else {
        return false;
    };
    if first_side != second_side || matches!(first_side, PortSide::East | PortSide::West) {
        return false;
    }

    let first_start = first_labels.position.x;
    let first_end = first_start + first_labels.size.width;
    let second_start = second_labels.position.x;
    let second_end = second_start + second_labels.size.width;

    first_start <= second_end && first_end >= second_start
}

fn self_loop_label_ids(
    holder: &SelfLoopHolder,
    first: usize,
    second: usize,
) -> Option<(usize, usize)> {
    Some((
        holder.hyper_loops.get(first)?.labels.as_ref()?.id?,
        holder.hyper_loops.get(second)?.labels.as_ref()?.id?,
    ))
}

fn self_loop_label_matrix_overlaps(
    holder: &SelfLoopHolder,
    label_crossing_matrix: &[Vec<bool>],
    first: usize,
    second: usize,
) -> bool {
    let Some((first_id, second_id)) = self_loop_label_ids(holder, first, second) else {
        return false;
    };
    label_crossing_matrix
        .get(first_id)
        .and_then(|row| row.get(second_id))
        .copied()
        .unwrap_or(false)
}

fn create_self_loop_crossing_graph(
    places: &PortPlaces,
    holder: &SelfLoopHolder,
    activity_over_ports: &[Vec<bool>],
    label_crossing_matrix: &[Vec<bool>],
) -> HyperEdgeGraph {
    let mut graph = HyperEdgeGraph::default();
    for _ in &holder.hyper_loops {
        graph.add_segment(HyperEdgeSegment::new());
    }

    for first in 0..holder.hyper_loops.len().saturating_sub(1) {
        for second in (first + 1)..holder.hyper_loops.len() {
            create_self_loop_slot_dependencies(
                places,
                holder,
                activity_over_ports,
                label_crossing_matrix,
                &mut graph,
                first,
                second,
            );
        }
    }

    graph
}

/// For each loop, which places of the node's port list the loop runs over.
pub(crate) fn compute_loop_activity(
    holder: &SelfLoopHolder,
    places: &PortPlaces,
) -> Vec<Vec<bool>> {
    let port_count = places.listed;
    holder
        .hyper_loops
        .iter()
        .map(|hyper_loop| {
            let mut activity = vec![false; port_count];
            let (Some(leftmost_port), Some(rightmost_port)) =
                (hyper_loop.leftmost_port, hyper_loop.rightmost_port)
            else {
                return activity;
            };
            // `port_count` is above 0 whenever the list has a place to find.
            let (Some(leftmost_port), Some(rightmost_port)) =
                (places.of(leftmost_port), places.of(rightmost_port))
            else {
                return activity;
            };

            let mut port = leftmost_port as isize - 1;
            while port != rightmost_port as isize {
                port = (port + 1).rem_euclid(port_count as isize);
                activity[port as usize] = true;
            }
            activity
        })
        .collect()
}

pub(crate) fn create_self_loop_slot_dependencies(
    places: &PortPlaces,
    holder: &SelfLoopHolder,
    activity_over_ports: &[Vec<bool>],
    label_crossing_matrix: &[Vec<bool>],
    graph: &mut HyperEdgeGraph,
    first: usize,
    second: usize,
) {
    let first_above_second = count_self_loop_slot_crossings(
        places,
        &holder.hyper_loops[first],
        &activity_over_ports[second],
    );
    let second_above_first = count_self_loop_slot_crossings(
        places,
        &holder.hyper_loops[second],
        &activity_over_ports[first],
    );

    if first_above_second < second_above_first {
        graph.add_regular_dependency(
            first,
            second,
            (second_above_first - first_above_second) as i32,
        );
    } else if second_above_first < first_above_second {
        graph.add_regular_dependency(
            second,
            first,
            (first_above_second - second_above_first) as i32,
        );
    } else if first_above_second != 0
        || self_loop_label_matrix_overlaps(holder, label_crossing_matrix, first, second)
    {
        graph.add_regular_dependency(first, second, 0);
        graph.add_regular_dependency(second, first, 0);
    }
}

fn count_self_loop_slot_crossings(
    places: &PortPlaces,
    upper_loop: &SelfHyperLoop,
    lower_loop_activity: &[bool],
) -> usize {
    upper_loop
        .ports
        .iter()
        .filter(|port| {
            places
                .of(port.port)
                .is_some_and(|place| lower_loop_activity.get(place).copied().unwrap_or(false))
        })
        .count()
}

fn assign_raw_routing_slots_to_segments(graph: &mut HyperEdgeGraph) {
    let mut sinks = VecDeque::new();

    for segment in 0..graph.segments.len() {
        graph.segments[segment].in_dep_weight = graph.segments[segment]
            .incoming_segment_dependencies
            .iter()
            .filter(|dependency| graph.dependencies[**dependency].source.is_some())
            .count() as i32;
        graph.segments[segment].out_dep_weight = graph.segments[segment]
            .outgoing_segment_dependencies
            .iter()
            .filter(|dependency| graph.dependencies[**dependency].target.is_some())
            .count() as i32;

        if graph.segments[segment].out_dep_weight == 0 {
            graph.segments[segment].routing_slot = 0;
            sinks.push_back(segment);
        }
    }

    while let Some(segment) = sinks.pop_front() {
        let next_routing_slot = graph.segments[segment].routing_slot + 1;
        for dependency in graph.segments[segment]
            .incoming_segment_dependencies
            .clone()
        {
            let Some(source) = graph.dependencies[dependency].source else {
                continue;
            };
            graph.segments[source].routing_slot =
                graph.segments[source].routing_slot.max(next_routing_slot);
            graph.segments[source].out_dep_weight -= 1;
            if graph.segments[source].out_dep_weight == 0 {
                sinks.push_back(source);
            }
        }
    }
}

fn assign_raw_routing_slots_to_loops(holder: &mut SelfLoopHolder, graph: &HyperEdgeGraph) {
    for (loop_index, hyper_loop) in holder.hyper_loops.iter_mut().enumerate() {
        let slot = graph.segments[loop_index].routing_slot.max(0) as usize;
        for side in hyper_loop.occupied_sides.clone() {
            hyper_loop.routing_slots[side.ordinal()] = slot;
        }
    }
}

fn shift_slots_towards_node(
    graph: &LGraph,
    holder: &mut SelfLoopHolder,
    activity_over_ports: &[Vec<bool>],
    label_crossing_matrix: &[Vec<bool>],
) {
    let port_count = activity_over_ports.iter().map(Vec::len).max().unwrap_or(0);
    let mut next_free_routing_slot_at_port = vec![0usize; port_count];

    for side in [
        PortSide::North,
        PortSide::East,
        PortSide::South,
        PortSide::West,
    ] {
        shift_slots_towards_node_on_side(
            graph,
            holder,
            activity_over_ports,
            side,
            &mut next_free_routing_slot_at_port,
            label_crossing_matrix,
        );
    }
}

fn shift_slots_towards_node_on_side(
    graph: &LGraph,
    holder: &mut SelfLoopHolder,
    activity_over_ports: &[Vec<bool>],
    side: PortSide,
    next_free_routing_slot_at_port: &mut [usize],
    label_crossing_matrix: &[Vec<bool>],
) {
    let mut loops = holder
        .hyper_loops
        .iter()
        .enumerate()
        .filter(|(_, hyper_loop)| hyper_loop.occupied_sides.contains(&side))
        .map(|(index, hyper_loop)| (index, hyper_loop.routing_slots[side.ordinal()]))
        .collect::<Vec<_>>();
    loops.sort_by_key(|(_, slot)| *slot);

    let Some((min_port, max_port)) = port_range_on_side(graph, holder.node, side) else {
        for (slot, (loop_index, _)) in loops.into_iter().enumerate() {
            holder.hyper_loops[loop_index].routing_slots[side.ordinal()] = slot;
        }
        return;
    };

    let mut slot_assigned_to_label = vec![None; label_crossing_matrix.len()];
    for (loop_index, _) in loops {
        let active_at_port = &activity_over_ports[loop_index];
        let mut lowest_available_slot = 0usize;
        for (port, next_free_slot) in next_free_routing_slot_at_port
            .iter()
            .enumerate()
            .take(max_port + 1)
            .skip(min_port)
        {
            if active_at_port.get(port).copied().unwrap_or(false) {
                lowest_available_slot = lowest_available_slot.max(*next_free_slot);
            }
        }

        if let Some(label_id) = holder.hyper_loops[loop_index]
            .labels
            .as_ref()
            .and_then(|labels| labels.id)
        {
            while label_crossing_matrix.get(label_id).is_some_and(|row| {
                row.iter().enumerate().any(|(other_label_id, overlaps)| {
                    *overlaps
                        && slot_assigned_to_label
                            .get(other_label_id)
                            .copied()
                            .flatten()
                            == Some(lowest_available_slot)
                })
            }) {
                lowest_available_slot += 1;
            }
        }

        holder.hyper_loops[loop_index].routing_slots[side.ordinal()] = lowest_available_slot;
        for (port, next_free_slot) in next_free_routing_slot_at_port
            .iter_mut()
            .enumerate()
            .take(max_port + 1)
            .skip(min_port)
        {
            if active_at_port.get(port).copied().unwrap_or(false) {
                *next_free_slot = lowest_available_slot + 1;
            }
        }
        if let Some(label_id) = holder.hyper_loops[loop_index]
            .labels
            .as_ref()
            .and_then(|labels| labels.id)
            && let Some(slot) = slot_assigned_to_label.get_mut(label_id)
        {
            *slot = Some(lowest_available_slot);
        }
    }
}

fn port_range_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> Option<(usize, usize)> {
    graph
        .layerless_nodes
        .get(node)?
        .ports
        .iter()
        .enumerate()
        .filter_map(|(port, port_data)| (port_data.side == side).then_some(port))
        .fold(None, |range, port| match range {
            Some((min, max)) => Some((min.min(port), max.max(port))),
            None => Some((port, port)),
        })
}
