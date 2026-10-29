//! Port of ELK's `HyperedgeCrossingsCounter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/HyperedgeCrossingsCounter.java

use std::collections::HashMap;

use crate::graph::{LGraph, NodeId, PortRef, PortSide};
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;

/// One hyperedge of `HyperedgeCrossingsCounter`: the ports its edges join, with the extreme
/// positions of those ports in each of the two layers.
#[derive(Debug, Default)]
pub(crate) struct Hyperedge {
    ports: Vec<PortRef>,
    upper_left: usize,
    lower_left: usize,
    upper_right: usize,
    lower_right: usize,
}

/// The corner sweep of `HyperedgeCrossingsCounter.countCrossings`: every hyperedge covers the
/// span `(upper, lower)` on one side. Each time a span closes, it crosses all spans still open.
/// Corners sort by position, then by opposite position, then upper before lower.
///
/// The sum can be negative, as ELK's `int` can: a span whose upper corner lies below its lower
/// corner closes before it opens.
pub(crate) fn count_overlapping_areas(spans: &[(usize, usize)]) -> isize {
    // (position, opposite position, is_lower)
    let mut corners = Vec::with_capacity(spans.len() * 2);
    for (upper, lower) in spans {
        corners.push((*upper, *lower, false));
        corners.push((*lower, *upper, true));
    }
    corners.sort();
    let mut open = 0isize;
    let mut crossings = 0isize;
    for (_, _, is_lower) in corners {
        if is_lower {
            open -= 1;
            crossings += open;
        } else {
            open += 1;
        }
    }
    crossings
}

impl CrossingsCounter {
    /// `HyperedgeCrossingsCounter.countCrossings`. The number is exact for plain edges and an
    /// estimate for hyperedges, as in ELK. Like ELK's `int`, it can be negative: a hyperedge with
    /// no port in one of the two layers has its upper corner below its lower corner.
    ///
    /// ELK breaks a tie between two hyperedges or corners by `hashCode()`. That tie cannot happen
    /// here: each hyperedge owns its own ports, so the positions it is sorted by never repeat.
    pub(crate) fn count_hyperedge_crossings(
        &mut self,
        graph: &LGraph,
        left_layer: &[NodeId],
        right_layer: &[NodeId],
    ) -> isize {
        let layer_of = |node: NodeId| graph.layerless_nodes[node].layer_index();
        let (Some(&left_first), Some(&right_first)) = (left_layer.first(), right_layer.first())
        else {
            return 0;
        };
        let left_layer_ref = layer_of(left_first);
        let right_layer_ref = layer_of(right_first);

        // Assign index values to the ports of the left layer, in the order north - east - south - west.
        let mut source_count = 0usize;
        for node in left_layer {
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter_with_ids() {
                let has_edge = port
                    .outgoing_edges()
                    .iter()
                    .any(|edge| layer_of(graph.edges[*edge].target().node) != layer_of(*node));
                if has_edge {
                    self.set_position(
                        graph,
                        PortRef {
                            node: *node,
                            port: port_index,
                        },
                        source_count,
                    );
                    source_count += 1;
                }
            }
        }

        // Assign index values to the ports of the right layer, in the order north - west - south - east.
        let mut target_count = 0usize;
        for node in right_layer {
            let ports = &graph.layerless_nodes[*node].ports;
            let enters_from_other_layer = |port: &crate::graph::LPort| {
                port.incoming_edges()
                    .iter()
                    .any(|edge| layer_of(graph.edges[*edge].source().node) != layer_of(*node))
            };
            // The leading north ports with an incoming edge.
            let north_input_ports = ports
                .iter()
                .take_while(|port| port.side == PortSide::North)
                .filter(|port| enters_from_other_layer(port))
                .count();
            let mut other_input_ports = 0usize;
            for (port_index, port) in ports.iter_with_ids().rev() {
                if !enters_from_other_layer(port) {
                    continue;
                }
                let key = PortRef {
                    node: *node,
                    port: port_index,
                };
                if port.side == PortSide::North {
                    self.set_position(graph, key, target_count);
                    target_count += 1;
                } else {
                    self.set_position(
                        graph,
                        key,
                        target_count + north_input_ports + other_input_ports,
                    );
                    other_input_ports += 1;
                }
            }
            target_count += other_input_ports;
        }

        // Gather hyperedges. `alive` keeps the insertion order of the ones not merged away.
        let mut hyperedges: Vec<Hyperedge> = Vec::new();
        let mut alive: Vec<usize> = Vec::new();
        let mut port_to_hyperedge: HashMap<PortRef, usize> = HashMap::new();
        for node in left_layer {
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter_with_ids() {
                let source_port = PortRef {
                    node: *node,
                    port: port_index,
                };
                for edge in port.outgoing_edges() {
                    let target_port = graph.edges[*edge].target();
                    if layer_of(*node) == layer_of(target_port.node) {
                        continue;
                    }
                    let source_he = port_to_hyperedge.get(&source_port).copied();
                    let target_he = port_to_hyperedge.get(&target_port).copied();
                    match (source_he, target_he) {
                        (None, None) => {
                            let he = hyperedges.len();
                            hyperedges.push(Hyperedge {
                                ports: vec![source_port, target_port],
                                ..Hyperedge::default()
                            });
                            alive.push(he);
                            port_to_hyperedge.insert(source_port, he);
                            port_to_hyperedge.insert(target_port, he);
                        }
                        (None, Some(he)) => {
                            hyperedges[he].ports.push(source_port);
                            port_to_hyperedge.insert(source_port, he);
                        }
                        (Some(he), None) => {
                            hyperedges[he].ports.push(target_port);
                            port_to_hyperedge.insert(target_port, he);
                        }
                        (Some(source_he), Some(target_he)) if source_he == target_he => {}
                        (Some(source_he), Some(target_he)) => {
                            let merged = std::mem::take(&mut hyperedges[target_he].ports);
                            for port in &merged {
                                port_to_hyperedge.insert(*port, source_he);
                            }
                            hyperedges[source_he].ports.extend(merged);
                            alive.retain(|he| *he != target_he);
                        }
                    }
                }
            }
        }

        // Determine top and bottom positions for each hyperedge.
        for he in &alive {
            let hyperedge = &mut hyperedges[*he];
            hyperedge.upper_left = source_count;
            hyperedge.upper_right = target_count;
            for port in &hyperedge.ports {
                let pos = self.position_of(graph, *port);
                let port_layer = layer_of(port.node);
                if port_layer == left_layer_ref {
                    hyperedge.upper_left = hyperedge.upper_left.min(pos);
                    hyperedge.lower_left = hyperedge.lower_left.max(pos);
                } else if port_layer == right_layer_ref {
                    hyperedge.upper_right = hyperedge.upper_right.min(pos);
                    hyperedge.lower_right = hyperedge.lower_right.max(pos);
                }
            }
        }

        // Determine the sequence of edge target positions sorted by source and target index.
        let mut sorted: Vec<&Hyperedge> = alive.iter().map(|he| &hyperedges[*he]).collect();
        sorted.sort_by(|a, b| {
            a.upper_left
                .cmp(&b.upper_left)
                .then(a.upper_right.cmp(&b.upper_right))
        });
        let mut south_sequence: Vec<usize> = sorted.iter().map(|he| he.upper_right).collect();
        let mut compress_deltas = vec![0isize; target_count + 1];
        for position in &south_sequence {
            compress_deltas[*position] = 1;
        }
        let mut delta = 0isize;
        for entry in &mut compress_deltas {
            if *entry == 1 {
                *entry = delta;
            } else {
                delta -= 1;
            }
        }
        let mut q = 0usize;
        for position in &mut south_sequence {
            *position = (*position as isize + compress_deltas[*position]) as usize;
            q = q.max(*position + 1);
        }

        // Build the accumulator tree.
        let mut first_index = 1usize;
        while first_index < q {
            first_index *= 2;
        }
        let tree_size = 2 * first_index - 1;
        first_index -= 1;
        let mut tree = vec![0usize; tree_size];

        // Count the straight-line crossings of the topmost edges.
        let mut crossings = 0isize;
        for position in &south_sequence {
            let mut index = position + first_index;
            tree[index] += 1;
            while index > 0 {
                if !index.is_multiple_of(2) {
                    crossings += tree[index + 1] as isize;
                }
                index = (index - 1) / 2;
                tree[index] += 1;
            }
        }

        // Count crossings caused by overlapping hyperedge areas, on each side.
        let left_corners = sorted
            .iter()
            .map(|he| (he.upper_left, he.lower_left))
            .collect::<Vec<_>>();
        let right_corners = sorted
            .iter()
            .map(|he| (he.upper_right, he.lower_right))
            .collect::<Vec<_>>();
        crossings += count_overlapping_areas(&left_corners);
        crossings += count_overlapping_areas(&right_corners);
        crossings
    }
}
