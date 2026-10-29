//! Port of ELK's `SwitchDecider`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/SwitchDecider.java
//!
//! `InLayerCrossingCounter` is the part of `CrossingsCounter` that the decider uses for edges
//! inside one layer.

use std::collections::{HashMap, HashSet};

use crate::graph::{LGraph, LNodeKind, NodeId, PortRef, PortSide};
use crate::intermediate::greedyswitch::crossing_matrix_filler::CrossingMatrixFiller;
use crate::intermediate::greedyswitch::north_south_edge_neighbouring_node_crossings_counter::NorthSouthEdgeNeighbouringNodeCrossingsCounter;
use crate::phase::ordering::counting::binary_indexed_tree::BinaryIndexedTree;
use crate::phase::ordering::counting::crossings_counter::ports_in_north_south_east_west_order;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;
use crate::phase::ordering::{
    connected_edges, is_edge_self_loop, is_in_layer_edge, is_long_edge_dummy, is_north_south_node,
    other_end_of_edge,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct InLayerCrossingCounter {
    port_positions: HashMap<PortRef, usize>,
    node_cardinalities: HashMap<NodeId, usize>,
    side: PortSide,
}

impl InLayerCrossingCounter {
    fn new(graph: &LGraph, free_layer: &[NodeId], side: PortSide) -> Self {
        let mut port_positions = HashMap::new();
        let mut node_cardinalities = HashMap::new();
        let mut position = 0usize;
        for node in free_layer {
            let ports = ports_for_counter_side(graph, *node, side, true);
            node_cardinalities.insert(*node, ports.len());
            for port in ports {
                port_positions.insert(port, position);
                position += 1;
            }
        }
        Self {
            port_positions,
            node_cardinalities,
            side,
        }
    }

    fn count_in_layer_crossings_between_nodes_in_both_orders(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) -> (usize, usize) {
        let mut ports =
            self.connected_in_layer_ports_sorted_by_position(graph, upper_node, lower_node);
        let upper_lower_crossings = self.count_in_layer_crossings_on_ports(graph, &ports);
        self.switch_nodes(graph, upper_node, lower_node);
        ports.sort_by_key(|port| self.position_of(*port));
        let lower_upper_crossings = self.count_in_layer_crossings_on_ports(graph, &ports);
        self.switch_nodes(graph, lower_node, upper_node);
        (upper_lower_crossings, lower_upper_crossings)
    }

    fn switch_nodes(&mut self, graph: &LGraph, was_upper_node: NodeId, was_lower_node: NodeId) {
        let lower_cardinality = *self.node_cardinalities.get(&was_lower_node).unwrap_or(&0);
        for port in ports_in_north_south_east_west_order(graph, was_upper_node, self.side) {
            if let Some(position) = self.port_positions.get_mut(&port) {
                *position += lower_cardinality;
            }
        }

        let upper_cardinality = *self.node_cardinalities.get(&was_upper_node).unwrap_or(&0);
        for port in ports_in_north_south_east_west_order(graph, was_lower_node, self.side) {
            if let Some(position) = self.port_positions.get_mut(&port) {
                *position = position.saturating_sub(upper_cardinality);
            }
        }
    }

    fn connected_in_layer_ports_sorted_by_position(
        &self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) -> Vec<PortRef> {
        let mut ports = HashSet::new();
        for node in [upper_node, lower_node] {
            for port in ports_in_north_south_east_west_order(graph, node, self.side) {
                for edge in connected_edges(graph, port) {
                    if is_edge_self_loop(graph, edge) {
                        continue;
                    }
                    ports.insert(port);
                    if is_in_layer_edge(graph, edge)
                        && let Some(other) = other_end_of_edge(graph, edge, port)
                    {
                        ports.insert(other);
                    }
                }
            }
        }

        let mut ports = ports.into_iter().collect::<Vec<_>>();
        ports.sort_by_key(|port| self.position_of(*port));
        ports
    }

    fn count_in_layer_crossings_on_ports(&self, graph: &LGraph, ports: &[PortRef]) -> usize {
        let mut crossings = 0usize;
        let mut ends = Vec::new();
        let mut index_tree = BinaryIndexedTree::new(self.port_positions.len());

        for port in ports {
            let Some(port_position) = self.position_of_checked(*port) else {
                continue;
            };
            index_tree.remove_all(port_position);
            let mut num_between_layer_edges = 0usize;

            for edge in connected_edges(graph, *port) {
                if is_in_layer_edge(graph, edge) {
                    let Some(other_end) = other_end_of_edge(graph, edge, *port) else {
                        continue;
                    };
                    let Some(end_position) = self.position_of_checked(other_end) else {
                        continue;
                    };
                    if end_position > port_position {
                        crossings += index_tree.rank(end_position).unwrap_or(0);
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

    fn position_of(&self, port: PortRef) -> usize {
        self.position_of_checked(port).unwrap_or(usize::MAX)
    }

    fn position_of_checked(&self, port: PortRef) -> Option<usize> {
        self.port_positions.get(&port).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchDecider {
    free_layer_index: usize,
    one_sided: bool,
    crossing_matrix_filler: CrossingMatrixFiller,
    left_in_layer_counter: InLayerCrossingCounter,
    right_in_layer_counter: InLayerCrossingCounter,
    north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter,
}

impl SwitchDecider {
    pub(crate) fn new(
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        crossing_matrix_filler: CrossingMatrixFiller,
        one_sided: bool,
    ) -> Self {
        let free_layer = &current_node_order[free_layer_index];
        Self {
            free_layer_index,
            one_sided,
            crossing_matrix_filler,
            left_in_layer_counter: InLayerCrossingCounter::new(graph, free_layer, PortSide::West),
            right_in_layer_counter: InLayerCrossingCounter::new(graph, free_layer, PortSide::East),
            north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(
                graph, free_layer,
            ),
        }
    }

    pub(crate) fn notify_of_switch(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.left_in_layer_counter
            .switch_nodes(graph, upper_node, lower_node);
        self.right_in_layer_counter
            .switch_nodes(graph, upper_node, lower_node);
        self.crossing_matrix_filler = CrossingMatrixFiller::new(
            current_node_order[self.free_layer_index].len(),
            if self.one_sided {
                CrossMinType::OneSidedGreedySwitch
            } else {
                CrossMinType::TwoSidedGreedySwitch
            },
            self.crossing_matrix_filler.direction,
        );
    }

    pub(crate) fn does_switch_reduce_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        upper_node_index: usize,
        lower_node_index: usize,
    ) -> bool {
        if self.constraints_prevent_switch(
            graph,
            &current_node_order[self.free_layer_index],
            upper_node_index,
            lower_node_index,
        ) {
            return false;
        }

        let free_layer = &current_node_order[self.free_layer_index];
        let upper_node = free_layer[upper_node_index];
        let lower_node = free_layer[lower_node_index];
        let layer_positions = layer_position_map(free_layer);

        let left_in_layer = self
            .left_in_layer_counter
            .count_in_layer_crossings_between_nodes_in_both_orders(graph, upper_node, lower_node);
        let right_in_layer = self
            .right_in_layer_counter
            .count_in_layer_crossings_between_nodes_in_both_orders(graph, upper_node, lower_node);
        self.north_south_counter
            .count_crossings(graph, upper_node, lower_node);

        let upper_lower_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(
            graph,
            current_node_order,
            self.free_layer_index,
            &layer_positions,
            upper_node,
            lower_node,
        ) + left_in_layer.0
            + right_in_layer.0
            + self.north_south_counter.upper_lower_crossings;
        let lower_upper_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(
            graph,
            current_node_order,
            self.free_layer_index,
            &layer_positions,
            lower_node,
            upper_node,
        ) + left_in_layer.1
            + right_in_layer.1
            + self.north_south_counter.lower_upper_crossings;

        upper_lower_crossings > lower_upper_crossings
    }

    pub(crate) fn constraints_prevent_switch(
        &self,
        graph: &LGraph,
        free_layer: &[NodeId],
        upper_node_index: usize,
        lower_node_index: usize,
    ) -> bool {
        let upper_node = free_layer[upper_node_index];
        let lower_node = free_layer[lower_node_index];
        have_successor_constraints(graph, upper_node, lower_node)
            || have_layout_unit_constraints(graph, upper_node, lower_node)
            || are_normal_and_north_south_port_dummy(graph, upper_node, lower_node)
    }
}

fn layer_position_map(layer: &[NodeId]) -> HashMap<NodeId, usize> {
    layer
        .iter()
        .copied()
        .enumerate()
        .map(|(position, node)| (node, position))
        .collect()
}

fn ports_for_counter_side(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
    top_down: bool,
) -> Vec<PortRef> {
    let mut ports = graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter_map(|(port_index, port)| {
            (port.side == side).then_some(PortRef {
                node,
                port: port_index,
            })
        })
        .collect::<Vec<_>>();
    let preserve_order =
        (side == PortSide::East && top_down) || (side == PortSide::West && !top_down);
    if !preserve_order {
        ports.reverse();
    }
    ports
}

fn are_normal_and_north_south_port_dummy(
    graph: &LGraph,
    upper_node: NodeId,
    lower_node: NodeId,
) -> bool {
    (is_north_south_node(graph, upper_node)
        && graph.layerless_nodes[lower_node].kind() == LNodeKind::Normal)
        || (is_north_south_node(graph, lower_node)
            && graph.layerless_nodes[upper_node].kind() == LNodeKind::Normal)
}

/// `SwitchDecider.haveSuccessorConstraints`.
pub(crate) fn have_successor_constraints(
    graph: &LGraph,
    upper_node: NodeId,
    lower_node: NodeId,
) -> bool {
    graph
        .in_layer_successor_constraints(upper_node)
        .contains(&lower_node)
}

/// `SwitchDecider.haveLayoutUnitConstraints`.
pub(crate) fn have_layout_unit_constraints(
    graph: &LGraph,
    upper_node: NodeId,
    lower_node: NodeId,
) -> bool {
    let neither_node_is_long_edge_dummy =
        !is_long_edge_dummy(graph, upper_node) && !is_long_edge_dummy(graph, lower_node);

    // If the nodes are part of a layout unit with more nodes than themselves, the layout units
    // must be equal for a switch to be allowed.
    let upper_layout_unit = graph.in_layer_layout_unit(upper_node);
    let lower_layout_unit = graph.in_layer_layout_unit(lower_node);
    let are_in_different_layout_units = upper_layout_unit != lower_layout_unit;

    let part_of_multi_node_layout_unit =
        |node: NodeId, unit: Option<NodeId>| unit.is_some_and(|unit| unit != node);
    let mut nodes_have_layout_units = part_of_multi_node_layout_unit(upper_node, upper_layout_unit)
        || part_of_multi_node_layout_unit(lower_node, lower_layout_unit);

    let upper_node_has_northern_edges = has_edges_on_side(graph, upper_node, PortSide::North);
    let lower_node_has_southern_edges = has_edges_on_side(graph, lower_node, PortSide::South);

    // ELK's hotfix for #162: if north or south edges are present, there must be a layout unit.
    nodes_have_layout_units |= has_edges_on_side(graph, upper_node, PortSide::South)
        || has_edges_on_side(graph, lower_node, PortSide::North);

    let has_layout_unit_constraint = (nodes_have_layout_units && are_in_different_layout_units)
        || (upper_node_has_northern_edges || lower_node_has_southern_edges);

    neither_node_is_long_edge_dummy && has_layout_unit_constraint
}

/// `SwitchDecider.hasEdgesOnSide`: whether a port of the side has an edge or a dummy that holds
/// its edges.
pub(crate) fn has_edges_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> bool {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter(|(_, port)| port.side == side)
        .any(|(index, port)| {
            port.port_dummy.is_some()
                || connected_edges(graph, PortRef { node, port: index })
                    .next()
                    .is_some()
        })
}
