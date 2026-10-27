//! Phase 3 crossing counters.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/BinaryIndexedTree.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/CrossingsCounter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/AllCrossingsCounter.java

use std::collections::HashMap;

use crate::graph::{LGraph, PortRef, PortSide};

/// `BinaryIndexedTree`.
///
/// elkjs does not check array bounds. Java would throw `ArrayIndexOutOfBoundsException` for an
/// index past the end; elkjs reads `undefined` and adds `NaN` to the sum, or grows the array. Both
/// happen when a counter reads the stale position of a port it did not number in this call. This
/// tree follows elkjs: `add` counts an index past the end in `size` and in nothing else, and `rank`
/// of such an index is `None`, which stands for `NaN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryIndexedTree {
    binary_sums: Vec<usize>,
    nums_per_index: Vec<usize>,
    size: usize,
}

impl BinaryIndexedTree {
    pub fn new(max_num: usize) -> Self {
        Self {
            binary_sums: vec![0; max_num + 1],
            nums_per_index: vec![0; max_num],
            size: 0,
        }
    }

    pub fn add(&mut self, index: usize) {
        self.size += 1;
        if let Some(entries) = self.nums_per_index.get_mut(index) {
            *entries += 1;
        }
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] += 1;
            i += i & i.wrapping_neg();
        }
    }

    /// The sum of the entries before `index`, or `None` when elkjs would get `NaN`.
    pub fn rank(&self, index: usize) -> Option<usize> {
        let mut i = index;
        let mut sum = Some(0usize);
        while i > 0 {
            sum = match (sum, self.binary_sums.get(i)) {
                (Some(sum), Some(entries)) => Some(sum + entries),
                _ => None,
            };
            i -= i & i.wrapping_neg();
        }
        sum
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn remove_all(&mut self, index: usize) {
        let Some(num_entries) = self.nums_per_index.get_mut(index) else {
            return;
        };
        let num_entries = std::mem::take(num_entries);
        if num_entries == 0 {
            return;
        }

        self.size -= num_entries;
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] -= num_entries;
            i += i & i.wrapping_neg();
        }
    }
}

/// `AllCrossingsCounter` with its `CrossingsCounter` and `HyperedgeCrossingsCounter`.
///
/// Both counters of ELK write the positions of ports into one `int[]` that lives as long as the
/// graph's `GraphInfoHolder`. A counter numbers only the ports it counts, and it reads the position
/// of the far end of each edge from that array. For an edge to a port that this call did not number,
/// such as an output on the west side of its node, the read gives the number an earlier call left
/// there, or 0 before any call. Keep one counter per graph, and never clear the positions.
///
/// ELK indexes the array by `port.id`, which `AllCrossingsCounter.initAtPortLevel` sets once. The
/// sweep reorders the ports of a node in place, so the index of a port in `node.ports` does not
/// stay. This counter indexes by `LPort::crossing_minimization_id`, which
/// `initialize_crossing_minimization_port_ids` sets once per graph and nothing changes after.
///
/// Counts are `f64`, as elkjs adds `NaN` for an index its trees do not have: see
/// [`BinaryIndexedTree`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossingsCounter {
    /// `portPos`, indexed by `crossing_minimization_id`. An id past the end reads as 0.
    port_positions: Vec<usize>,
}

impl CrossingsCounter {
    pub fn count_crossings_between_layers(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[usize],
        right_layer_nodes: &[usize],
    ) -> f64 {
        let ports =
            self.init_port_positions_counter_clockwise(graph, left_layer_nodes, right_layer_nodes);
        let mut index_tree = BinaryIndexedTree::new(ports.len());
        self.count_crossings_on_ports(graph, &ports, &mut index_tree)
    }

    #[cfg(test)]
    pub fn count_all_crossings(&mut self, graph: &LGraph) -> f64 {
        // A graph built by hand has no port ids yet; the sweep sets them before any count.
        let mut graph = graph.clone();
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        let graph = &graph;
        let current_order = graph
            .layers
            .iter()
            .map(|layer| layer.nodes.clone())
            .collect::<Vec<_>>();
        self.count_all_crossings_in_order(graph, &current_order)
    }

    pub fn count_all_crossings_in_order(
        &mut self,
        graph: &LGraph,
        current_order: &[Vec<usize>],
    ) -> f64 {
        if current_order.is_empty() {
            return 0.0;
        }

        let has_hyperedges_east_of_index = has_hyperedges_east_of_index(graph, current_order);
        let mut crossings =
            self.count_in_layer_crossings_on_side(graph, &current_order[0], PortSide::West);
        if current_order.len() > 1 {
            crossings += self.count_in_layer_crossings_on_side(
                graph,
                &current_order[current_order.len() - 1],
                PortSide::East,
            );
        }
        for layer_index in 0..current_order.len().saturating_sub(1) {
            let left_layer = &current_order[layer_index];
            let right_layer = &current_order[layer_index + 1];
            // AllCrossingsCounter.countCrossingsAt: a layer pair with a port that has more than
            // one edge goes to the hyperedge counter, which also needs the in-layer counts.
            if has_hyperedges_east_of_index[layer_index] {
                crossings += self.count_hyperedge_crossings(graph, left_layer, right_layer) as f64;
                crossings +=
                    self.count_in_layer_crossings_on_side(graph, left_layer, PortSide::East);
                crossings +=
                    self.count_in_layer_crossings_on_side(graph, right_layer, PortSide::West);
            } else {
                crossings += self.count_crossings_between_layers(graph, left_layer, right_layer);
            }
        }
        crossings
    }

    pub fn count_in_layer_crossings_on_side(
        &mut self,
        graph: &LGraph,
        nodes: &[usize],
        side: PortSide,
    ) -> f64 {
        let ports = self.init_port_positions_for_in_layer_crossings(graph, nodes, side);
        let mut index_tree = BinaryIndexedTree::new(ports.len());
        self.count_in_layer_crossings_on_ports(graph, &ports, &mut index_tree)
    }

    fn init_port_positions_counter_clockwise(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[usize],
        right_layer_nodes: &[usize],
    ) -> Vec<PortRef> {
        let mut ports = Vec::new();
        self.init_positions(graph, left_layer_nodes, &mut ports, PortSide::East, true);
        self.init_positions(graph, right_layer_nodes, &mut ports, PortSide::West, false);
        ports
    }

    fn init_positions(
        &mut self,
        graph: &LGraph,
        nodes: &[usize],
        ports: &mut Vec<PortRef>,
        side: PortSide,
        top_down: bool,
    ) {
        let node_iter: Box<dyn Iterator<Item = usize> + '_> = if top_down {
            Box::new(nodes.iter().copied())
        } else {
            Box::new(nodes.iter().rev().copied())
        };

        for node in node_iter {
            for port in ports_for_side(graph, node, side, top_down) {
                let position = ports.len();
                self.set_position(graph, port, position);
                ports.push(port);
            }
        }
    }

    fn count_crossings_on_ports(
        &self,
        graph: &LGraph,
        ports: &[PortRef],
        index_tree: &mut BinaryIndexedTree,
    ) -> f64 {
        let mut crossings = 0.0f64;
        let mut ends = Vec::new();

        for port in ports {
            let port_position = self.position_of(graph, *port);
            index_tree.remove_all(port_position);

            for edge in connected_edges(graph, *port) {
                let Some(other_end) = other_end_of(graph, edge, *port) else {
                    continue;
                };
                let end_position = self.position_of(graph, other_end);
                if end_position > port_position {
                    crossings += rank_or_nan(index_tree.rank(end_position));
                    ends.push(end_position);
                }
            }

            while let Some(end) = ends.pop() {
                index_tree.add(end);
            }
        }

        crossings
    }

    fn init_port_positions_for_in_layer_crossings(
        &mut self,
        graph: &LGraph,
        nodes: &[usize],
        side: PortSide,
    ) -> Vec<PortRef> {
        let mut ports = Vec::new();
        for node in nodes {
            for port in ports_in_north_south_east_west_order(graph, *node, side) {
                let position = ports.len();
                self.set_position(graph, port, position);
                ports.push(port);
            }
        }
        ports
    }

    fn count_in_layer_crossings_on_ports(
        &self,
        graph: &LGraph,
        ports: &[PortRef],
        index_tree: &mut BinaryIndexedTree,
    ) -> f64 {
        let mut crossings = 0.0f64;
        let mut ends = Vec::new();

        for port in ports {
            let port_position = self.position_of(graph, *port);
            index_tree.remove_all(port_position);
            let mut num_between_layer_edges = 0usize;

            for edge in connected_edges(graph, *port) {
                if is_in_layer_edge(graph, edge) {
                    let Some(other_end) = other_end_of(graph, edge, *port) else {
                        continue;
                    };
                    let end_position = self.position_of(graph, other_end);
                    if end_position > port_position {
                        crossings += rank_or_nan(index_tree.rank(end_position));
                        ends.push(end_position);
                    }
                } else {
                    num_between_layer_edges += 1;
                }
            }

            crossings += (index_tree.size() * num_between_layer_edges) as f64;
            while let Some(end) = ends.pop() {
                index_tree.add(end);
            }
        }

        crossings
    }

    /// `CrossingsCounter.positionOf`: a port nobody numbered yet is at 0.
    fn position_of(&self, graph: &LGraph, port: PortRef) -> usize {
        port_id(graph, port)
            .and_then(|id| self.port_positions.get(id).copied())
            .unwrap_or(0)
    }

    /// `portPositions[port.id] = position`.
    fn set_position(&mut self, graph: &LGraph, port: PortRef, position: usize) {
        let Some(id) = port_id(graph, port) else {
            return;
        };
        if self.port_positions.len() <= id {
            self.port_positions.resize(id + 1, 0);
        }
        self.port_positions[id] = position;
    }

    /// `HyperedgeCrossingsCounter.countCrossings`. The number is exact for plain edges and an
    /// estimate for hyperedges, as in ELK.
    ///
    /// ELK breaks a tie between two hyperedges or corners by `hashCode()`. That tie cannot happen
    /// here: each hyperedge owns its own ports, so the positions it is sorted by never repeat.
    fn count_hyperedge_crossings(
        &mut self,
        graph: &LGraph,
        left_layer: &[usize],
        right_layer: &[usize],
    ) -> usize {
        let layer_of = |node: usize| graph.layerless_nodes[node].layer_index;
        let (Some(&left_first), Some(&right_first)) = (left_layer.first(), right_layer.first())
        else {
            return 0;
        };
        let left_layer_ref = layer_of(left_first);
        let right_layer_ref = layer_of(right_first);

        // Assign index values to the ports of the left layer, in the order north - east - south - west.
        let mut source_count = 0usize;
        for node in left_layer {
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter().enumerate() {
                let has_edge = port
                    .outgoing_edges
                    .iter()
                    .any(|edge| layer_of(graph.edges[*edge].target.node) != layer_of(*node));
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
                port.incoming_edges
                    .iter()
                    .any(|edge| layer_of(graph.edges[*edge].source.node) != layer_of(*node))
            };
            // The leading north ports with an incoming edge.
            let north_input_ports = ports
                .iter()
                .take_while(|port| port.side == PortSide::North)
                .filter(|port| enters_from_other_layer(port))
                .count();
            let mut other_input_ports = 0usize;
            for (port_index, port) in ports.iter().enumerate().rev() {
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
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter().enumerate() {
                let source_port = PortRef {
                    node: *node,
                    port: port_index,
                };
                for edge in &port.outgoing_edges {
                    let target_port = graph.edges[*edge].target;
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
        let mut crossings = 0usize;
        for position in &south_sequence {
            let mut index = position + first_index;
            tree[index] += 1;
            while index > 0 {
                if !index.is_multiple_of(2) {
                    crossings += tree[index + 1];
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

/// `port.id`. Every port of a graph in the sweep has one, see `CrossingsCounter`.
fn port_id(graph: &LGraph, port: PortRef) -> Option<usize> {
    let id = graph.layerless_nodes[port.node].ports[port.port].crossing_minimization_id;
    debug_assert!(id.is_some(), "a port in the sweep has no crossing id");
    id
}

fn rank_or_nan(rank: Option<usize>) -> f64 {
    rank.map_or(f64::NAN, |rank| rank as f64)
}

/// `AllCrossingsCounter.initAtPortLevel`: `hasHyperEdgesEastOfIndex[l]` is set when an east port of
/// layer `l`, or a west port of layer `l + 1`, has more than one edge. Only real ports can have
/// that; a port ELK makes up for a plain edge has exactly one.
fn has_hyperedges_east_of_index(graph: &LGraph, order: &[Vec<usize>]) -> Vec<bool> {
    let mut flags = vec![false; order.len()];
    for (layer_index, layer) in order.iter().enumerate() {
        for node in layer {
            for port in &graph.layerless_nodes[*node].ports {
                if port.outgoing_edges.len() + port.incoming_edges.len() <= 1 {
                    continue;
                }
                if port.side == PortSide::East {
                    flags[layer_index] = true;
                } else if port.side == PortSide::West && layer_index > 0 {
                    flags[layer_index - 1] = true;
                }
            }
        }
    }
    flags
}

/// One hyperedge of `HyperedgeCrossingsCounter`: the ports its edges join, with the extreme
/// positions of those ports in each of the two layers.
#[derive(Debug, Default)]
struct Hyperedge {
    ports: Vec<PortRef>,
    upper_left: usize,
    lower_left: usize,
    upper_right: usize,
    lower_right: usize,
}

/// The corner sweep of `HyperedgeCrossingsCounter.countCrossings`: every hyperedge covers the
/// span `(upper, lower)` on one side. Each time a span closes, it crosses all spans still open.
/// Corners sort by position, then by opposite position, then upper before lower.
fn count_overlapping_areas(spans: &[(usize, usize)]) -> usize {
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
    crossings.max(0) as usize
}

pub fn ports_in_north_south_east_west_order(
    graph: &LGraph,
    node: usize,
    side: PortSide,
) -> Vec<PortRef> {
    let mut ports = graph.layerless_nodes[node]
        .ports
        .iter()
        .enumerate()
        .filter_map(|(port_index, port)| {
            (port.side == side).then_some(PortRef {
                node,
                port: port_index,
            })
        })
        .collect::<Vec<_>>();
    if matches!(side, PortSide::South | PortSide::West) {
        ports.reverse();
    }
    ports
}

fn ports_for_side(graph: &LGraph, node: usize, side: PortSide, top_down: bool) -> Vec<PortRef> {
    let ports = graph.layerless_nodes[node]
        .ports
        .iter()
        .enumerate()
        .filter_map(|(port_index, port)| {
            (port.side == side).then_some(PortRef {
                node,
                port: port_index,
            })
        });

    let mut ports = ports.collect::<Vec<_>>();
    let preserve_order =
        (side == PortSide::East && top_down) || (side == PortSide::West && !top_down);
    if !preserve_order {
        ports.reverse();
    }
    ports
}

fn is_in_layer_edge(graph: &LGraph, edge: usize) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    graph.layerless_nodes[edge.source.node].layer_index
        == graph.layerless_nodes[edge.target.node].layer_index
}

fn connected_edges(graph: &LGraph, port: PortRef) -> impl Iterator<Item = usize> + '_ {
    graph.layerless_nodes[port.node].ports[port.port]
        .incoming_edges
        .iter()
        .chain(
            graph.layerless_nodes[port.node].ports[port.port]
                .outgoing_edges
                .iter(),
        )
        .copied()
}

fn other_end_of(graph: &LGraph, edge: usize, port: PortRef) -> Option<PortRef> {
    let edge = graph.edges.get(edge)?;
    if edge.source == port {
        Some(edge.target)
    } else if edge.target == port {
        Some(edge.source)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LGraph, LNode, LPoint, LayeredEdge, PortType};
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::options::{ElkDirection, LayeredOptions};
    use crate::p2layers::layer_network_simplex;
    use crate::p3order::{process_port_sides, sort_port_lists};

    fn node(id: &str) -> ElkInputNode {
        ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: id.to_string(),
            width: 80.0,
            height: 40.0,
            parent: None,
            direction: None,
            hierarchy_handling: None,
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::FIXED,
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: None,
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
        }
    }

    fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
        ElkInputEdge {
            source_port: None,
            target_port: None,
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            labels: Vec::new(),
            minlen: 1,
            inside_self_loops_yo: false,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            container: None,
        }
    }

    fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> LGraph {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap();
        layer_network_simplex(&mut graph).unwrap();
        process_port_sides(&mut graph);
        sort_port_lists(&mut graph);
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        graph
    }

    fn layered_edge(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
        LayeredEdge {
            id: id.to_string(),
            source,
            target,
            source_node_id: String::new(),
            target_node_id: String::new(),
            labels: Vec::new(),
            minlen: 1,
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            thickness: 1.0,
            original_opposite_port: None,
            compound_segment: None,
        }
    }

    #[test]
    fn binary_indexed_tree_counts_entries_before_index() {
        let mut tree = BinaryIndexedTree::new(5);
        tree.add(3);
        tree.add(1);
        tree.add(3);

        assert_eq!(tree.rank(3), Some(1));
        assert_eq!(tree.rank(4), Some(3));
        assert_eq!(tree.size(), 3);

        tree.remove_all(3);

        assert_eq!(tree.rank(4), Some(1));
        assert_eq!(tree.size(), 1);
    }

    #[test]
    fn crossing_counter_counts_between_layer_inversions() {
        let mut graph = graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Top")
            .unwrap();
        let bottom = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Bottom")
            .unwrap();
        let left = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Left")
            .unwrap();
        let right = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Right")
            .unwrap();
        graph.layers[0].nodes = vec![top, bottom];
        graph.layers[1].nodes = vec![left, right];

        let crossings = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(crossings, 1.0);
    }

    #[test]
    fn crossing_counter_reports_zero_for_non_crossing_between_layer_edges() {
        let mut graph = graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Left", "Top", "Left"),
                edge("Bottom-Right", "Bottom", "Right"),
            ],
        );
        let top = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Top")
            .unwrap();
        let bottom = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Bottom")
            .unwrap();
        let left = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Left")
            .unwrap();
        let right = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Right")
            .unwrap();
        graph.layers[0].nodes = vec![top, bottom];
        graph.layers[1].nodes = vec![left, right];

        let crossings = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(crossings, 0.0);
    }

    #[test]
    fn crossing_counter_detects_target_layer_order_improvement() {
        let mut graph = graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Top")
            .unwrap();
        let bottom = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Bottom")
            .unwrap();
        let left = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Left")
            .unwrap();
        let right = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Right")
            .unwrap();
        graph.layers[0].nodes = vec![top, bottom];
        graph.layers[1].nodes = vec![left, right];

        let crossing_order = CrossingsCounter::default().count_all_crossings(&graph);
        graph.layers[1].nodes = vec![right, left];
        let improved_order = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(crossing_order, 1.0);
        assert_eq!(improved_order, 0.0);
    }

    #[test]
    fn crossing_counter_reuse_matches_a_fresh_counter_after_order_changes() {
        let graph = graph(
            vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
            vec![
                edge("Top-Right", "Top", "Right"),
                edge("Bottom-Left", "Bottom", "Left"),
            ],
        );
        let top = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Top")
            .unwrap();
        let bottom = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Bottom")
            .unwrap();
        let left = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Left")
            .unwrap();
        let right = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "Right")
            .unwrap();
        let crossing_order = vec![vec![top, bottom], vec![left, right]];
        let non_crossing_order = vec![vec![top, bottom], vec![right, left]];

        let mut reused = CrossingsCounter::default();
        assert_eq!(
            reused.count_all_crossings_in_order(&graph, &crossing_order),
            1.0
        );
        assert_eq!(
            reused.count_all_crossings_in_order(&graph, &non_crossing_order),
            0.0
        );

        let reused_result = reused.count_all_crossings_in_order(&graph, &crossing_order);
        let fresh_result =
            CrossingsCounter::default().count_all_crossings_in_order(&graph, &crossing_order);
        assert_eq!(reused_result, fresh_result);
    }

    // Mutation: clamp the index in `BinaryIndexedTree::rank` to the last one, as the tree did, so a
    // rank past the end is a number.
    #[test]
    fn a_rank_past_the_end_of_the_tree_is_nan() {
        let mut tree = BinaryIndexedTree::new(3);
        tree.add(1);
        // The last index the tree holds, and the one after it.
        assert_eq!(tree.rank(3), Some(1));
        assert_eq!(tree.rank(4), None);
        assert_eq!(tree.rank(40), None);
    }

    // Mutation: ignore an `add` past the end, as the tree did, so it does not count in `size`.
    #[test]
    fn an_entry_past_the_end_of_the_tree_counts_in_its_size_only() {
        let mut tree = BinaryIndexedTree::new(2);
        tree.add(7);
        assert_eq!(tree.size(), 1);
        assert_eq!(tree.rank(2), Some(0));
        tree.add(1);
        assert_eq!(tree.size(), 2);
        assert_eq!(tree.rank(2), Some(1));
    }

    /// Two nodes in the first layer, joined to the west ports of a node in the second layer, and
    /// a third west port of that node that leaves for a node in the third layer. The pair of the
    /// last two layers numbers only the port of the last node, so the far end of that edge keeps the
    /// position the first pair gave it.
    fn graph_with_a_west_port_that_leaves() -> (LGraph, Vec<Vec<usize>>) {
        graph_with_a_west_port_that_leaves_and_idle_ports_on_b(0)
    }

    /// The same graph, with `idle` more west ports on the last node. They have no edge. They make
    /// the tree of the last pair as big as the number of its ports.
    fn graph_with_a_west_port_that_leaves_and_idle_ports_on_b(
        idle: usize,
    ) -> (LGraph, Vec<Vec<usize>>) {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for id in ["a1", "a2", "n", "b"] {
            graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
        }
        for (node, layer) in [(0, 0), (1, 0), (2, 1), (3, 2)] {
            graph.set_node_layer(node, layer);
        }
        let out_of = |graph: &mut LGraph, node| {
            graph
                .add_port(node, PortType::Output, PortSide::East, LPoint::default())
                .unwrap()
        };
        let into = |graph: &mut LGraph, node, side| {
            graph
                .add_port(node, PortType::Input, side, LPoint::default())
                .unwrap()
        };
        let (a1_out, a2_out) = (out_of(&mut graph, 0), out_of(&mut graph, 1));
        let (n_in1, n_in2) = (
            into(&mut graph, 2, PortSide::West),
            into(&mut graph, 2, PortSide::West),
        );
        let n_leaves = graph
            .add_port(2, PortType::Output, PortSide::West, LPoint::default())
            .unwrap();
        let b_in = into(&mut graph, 3, PortSide::West);
        for _ in 0..idle {
            into(&mut graph, 3, PortSide::West);
        }
        graph.add_edge(layered_edge("a1-n", a1_out, n_in1));
        graph.add_edge(layered_edge("a2-n", a2_out, n_in2));
        graph.add_edge(layered_edge("n-b", n_leaves, b_in));
        let order = vec![vec![0, 1], vec![2], vec![3]];
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        (graph, order)
    }

    // Mutation: clear the port positions at the start of `init_port_positions_counter_clockwise`,
    // as the counter did. The edge from the west port then reads position 0 and the count is a
    // number.
    #[test]
    fn the_far_end_of_an_edge_from_an_unnumbered_port_has_a_stale_position() {
        let (graph, order) = graph_with_a_west_port_that_leaves();
        let mut counter = CrossingsCounter::default();
        // Alone, the last pair finds no position for the west port: it is at 0.
        assert_eq!(
            counter.count_crossings_between_layers(&graph, &order[1], &order[2]),
            0.0
        );
        // The first pair gives that port position 4. The tree of the last pair holds one port.
        counter.count_crossings_between_layers(&graph, &order[0], &order[1]);
        assert!(
            counter
                .count_crossings_between_layers(&graph, &order[1], &order[2])
                .is_nan()
        );
        // The count of the whole graph is NaN too.
        assert!(
            CrossingsCounter::default()
                .count_all_crossings_in_order(&graph, &order)
                .is_nan()
        );
    }

    // Mutation: clear the port positions at the end of `count_all_crossings_in_order`. A count of
    // the whole graph then leaves no positions behind for the next one.
    #[test]
    fn a_counter_keeps_the_positions_of_its_earlier_counts() {
        let (graph, order) = graph_with_a_west_port_that_leaves();
        let mut counter = CrossingsCounter::default();
        counter.count_all_crossings_in_order(&graph, &order);
        // The order of the layers is the same, so every count meets the same stale position.
        assert!(
            counter
                .count_crossings_between_layers(&graph, &order[1], &order[2])
                .is_nan()
        );
    }

    // Mutation: index `port_positions` by the place of the port in `node.ports` instead of by its
    // `crossing_minimization_id`. The reorder then hands the stale position of one port to another.
    #[test]
    fn a_stale_position_stays_with_its_port_when_the_sweep_reorders_the_ports() {
        let (mut graph, order) = graph_with_a_west_port_that_leaves_and_idle_ports_on_b(2);
        let mut counter = CrossingsCounter::default();
        // The first pair numbers the three west ports of `n` as 2, 3 and 4. The port that leaves
        // for `b` is the last one, at 4.
        counter.count_crossings_between_layers(&graph, &order[0], &order[1]);
        // The sweep moves that port to the front of `n`. It keeps its id and so its position 4.
        assert!(graph.reorder_node_ports(2, [2, 0, 1]));
        // The last pair holds three ports, so its tree has no place 4: the count is NaN. The port
        // that now sits at the old place of the front port would read 2 and give 0.
        assert!(
            counter
                .count_crossings_between_layers(&graph, &order[1], &order[2])
                .is_nan()
        );
    }

    #[test]
    fn crossing_counter_counts_crossing_ports_between_same_two_nodes() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("source", 10.0, 10.0, None));
        graph
            .layerless_nodes
            .push(LNode::new("target", 10.0, 10.0, None));
        graph.set_node_layer(0, 0);
        graph.set_node_layer(1, 1);

        let source_top = graph
            .add_port(0, PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let source_bottom = graph
            .add_port(0, PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let target_bottom = graph
            .add_port(1, PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let target_top = graph
            .add_port(1, PortType::Input, PortSide::West, LPoint::default())
            .unwrap();

        graph.add_edge(layered_edge("top-to-bottom", source_top, target_bottom));
        graph.add_edge(layered_edge("bottom-to-top", source_bottom, target_top));

        let crossings = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(crossings, 1.0);
    }
}
