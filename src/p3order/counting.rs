//! Phase 3 crossing counters.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/BinaryIndexedTree.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/CrossingsCounter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/AllCrossingsCounter.java

use std::collections::HashMap;

use crate::graph::{LGraph, PortRef, PortSide};

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

    pub fn add(&mut self, index: usize) -> bool {
        if index >= self.nums_per_index.len() {
            return false;
        }

        self.size += 1;
        self.nums_per_index[index] += 1;
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] += 1;
            i += i & i.wrapping_neg();
        }
        true
    }

    pub fn rank(&self, index: usize) -> usize {
        let mut i = index.min(self.binary_sums.len().saturating_sub(1));
        let mut sum = 0usize;
        while i > 0 {
            sum += self.binary_sums[i];
            i -= i & i.wrapping_neg();
        }
        sum
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn remove_all(&mut self, index: usize) -> bool {
        let Some(num_entries) = self.nums_per_index.get_mut(index) else {
            return false;
        };
        let num_entries = *num_entries;
        if num_entries == 0 {
            return true;
        }

        self.nums_per_index[index] = 0;
        self.size -= num_entries;
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] -= num_entries;
            i += i & i.wrapping_neg();
        }
        true
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossingsCounter {
    port_positions: HashMap<PortRef, usize>,
}

impl CrossingsCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count_crossings_between_layers(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[usize],
        right_layer_nodes: &[usize],
    ) -> usize {
        let ports =
            self.init_port_positions_counter_clockwise(graph, left_layer_nodes, right_layer_nodes);
        let mut index_tree = BinaryIndexedTree::new(ports.len());
        self.count_crossings_on_ports(graph, &ports, &mut index_tree)
    }

    #[cfg(test)]
    pub fn count_all_crossings(&mut self, graph: &LGraph) -> usize {
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
    ) -> usize {
        if current_order.is_empty() {
            return 0;
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
                crossings += count_hyperedge_crossings(graph, left_layer, right_layer);
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
    ) -> usize {
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
        self.port_positions.clear();
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
                self.port_positions.insert(port, position);
                ports.push(port);
            }
        }
    }

    fn count_crossings_on_ports(
        &self,
        graph: &LGraph,
        ports: &[PortRef],
        index_tree: &mut BinaryIndexedTree,
    ) -> usize {
        let mut crossings = 0usize;
        let mut ends = Vec::new();

        for port in ports {
            let Some(port_position) = self.position_of(*port) else {
                continue;
            };
            index_tree.remove_all(port_position);

            for edge in connected_edges(graph, *port) {
                let Some(other_end) = other_end_of(graph, edge, *port) else {
                    continue;
                };
                let Some(end_position) = self.position_of(other_end) else {
                    continue;
                };
                if end_position > port_position {
                    crossings += index_tree.rank(end_position);
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
        self.port_positions.clear();
        for node in nodes {
            for port in ports_in_north_south_east_west_order(graph, *node, side) {
                let position = ports.len();
                self.port_positions.insert(port, position);
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
    ) -> usize {
        let mut crossings = 0usize;
        let mut ends = Vec::new();

        for port in ports {
            let Some(port_position) = self.position_of(*port) else {
                continue;
            };
            index_tree.remove_all(port_position);
            let mut num_between_layer_edges = 0usize;

            for edge in connected_edges(graph, *port) {
                if is_in_layer_edge(graph, edge) {
                    let Some(other_end) = other_end_of(graph, edge, *port) else {
                        continue;
                    };
                    let Some(end_position) = self.position_of(other_end) else {
                        continue;
                    };
                    if end_position > port_position {
                        crossings += index_tree.rank(end_position);
                        ends.push(end_position);
                    }
                } else {
                    num_between_layer_edges += 1;
                }
            }

            crossings += index_tree.size() * num_between_layer_edges;
            while let Some(end) = ends.pop() {
                index_tree.add(end);
            }
        }

        crossings
    }

    fn position_of(&self, port: PortRef) -> Option<usize> {
        self.port_positions.get(&port).copied()
    }
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

/// `HyperedgeCrossingsCounter.countCrossings`. The number is exact for plain edges and an
/// estimate for hyperedges, as in ELK.
///
/// ELK breaks a tie between two hyperedges or corners by `hashCode()`. That tie cannot happen
/// here: each hyperedge owns its own ports, so the positions it is sorted by never repeat.
fn count_hyperedge_crossings(graph: &LGraph, left_layer: &[usize], right_layer: &[usize]) -> usize {
    let layer_of = |node: usize| graph.layerless_nodes[node].layer_index;
    let (Some(&left_first), Some(&right_first)) = (left_layer.first(), right_layer.first()) else {
        return 0;
    };
    let left_layer_ref = layer_of(left_first);
    let right_layer_ref = layer_of(right_first);
    let mut port_pos: HashMap<PortRef, usize> = HashMap::new();

    // Assign index values to the ports of the left layer, in the order north - east - south - west.
    let mut source_count = 0usize;
    for node in left_layer {
        for (port_index, port) in graph.layerless_nodes[*node].ports.iter().enumerate() {
            let has_edge = port
                .outgoing_edges
                .iter()
                .any(|edge| layer_of(graph.edges[*edge].target.node) != layer_of(*node));
            if has_edge {
                port_pos.insert(
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
                port_pos.insert(key, target_count);
                target_count += 1;
            } else {
                port_pos.insert(key, target_count + north_input_ports + other_input_ports);
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
            let Some(&pos) = port_pos.get(port) else {
                continue;
            };
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

        assert_eq!(tree.rank(3), 1);
        assert_eq!(tree.rank(4), 3);
        assert_eq!(tree.size(), 3);

        tree.remove_all(3);

        assert_eq!(tree.rank(4), 1);
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

        let crossings = CrossingsCounter::new().count_all_crossings(&graph);

        assert_eq!(crossings, 1);
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

        let crossings = CrossingsCounter::new().count_all_crossings(&graph);

        assert_eq!(crossings, 0);
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

        let crossing_order = CrossingsCounter::new().count_all_crossings(&graph);
        graph.layers[1].nodes = vec![right, left];
        let improved_order = CrossingsCounter::new().count_all_crossings(&graph);

        assert_eq!(crossing_order, 1);
        assert_eq!(improved_order, 0);
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

        let mut reused = CrossingsCounter::new();
        assert_eq!(
            reused.count_all_crossings_in_order(&graph, &crossing_order),
            1
        );
        assert_eq!(
            reused.count_all_crossings_in_order(&graph, &non_crossing_order),
            0
        );

        let reused_result = reused.count_all_crossings_in_order(&graph, &crossing_order);
        let fresh_result =
            CrossingsCounter::new().count_all_crossings_in_order(&graph, &crossing_order);
        assert_eq!(reused_result, fresh_result);
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

        let crossings = CrossingsCounter::new().count_all_crossings(&graph);

        assert_eq!(crossings, 1);
    }
}
