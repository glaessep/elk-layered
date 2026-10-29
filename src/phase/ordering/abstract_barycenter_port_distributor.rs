//! Port of ELK's `AbstractBarycenterPortDistributor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/AbstractBarycenterPortDistributor.java
//!
//! Its two subclasses, `NodeRelativePortDistributor` and `LayerTotalPortDistributor`, differ only in
//! `calculatePortRanks`. `PortDistributorKind` picks the one to use, so both live here.

use std::collections::HashMap;

use crate::graph::{LGraph, NodeId, PortRef, PortSide, PortType};
use crate::phase::ordering::counting::crossings_counter::port_degree;
use crate::phase::ordering::sweep_port_distributor::PortDistributorKind;
use crate::phase::ordering::{connected_edges, is_first_layer, other_end_of_edge};

#[derive(Debug, Clone)]
pub(crate) struct BarycenterPortDistributor {
    kind: PortDistributorKind,
    port_ranks: HashMap<usize, f64>,
    port_barycenters: HashMap<usize, f64>,
    node_positions: HashMap<NodeId, usize>,
    min_barycenter: f64,
    max_barycenter: f64,
}

impl BarycenterPortDistributor {
    pub(crate) fn new(kind: PortDistributorKind, order: &[Vec<NodeId>]) -> Self {
        let node_positions = order
            .iter()
            .flat_map(|layer| {
                layer
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(position, node)| (node, position))
            })
            .collect::<HashMap<_, _>>();
        Self {
            kind,
            port_ranks: HashMap::new(),
            port_barycenters: HashMap::new(),
            node_positions,
            min_barycenter: 0.0,
            max_barycenter: 0.0,
        }
    }

    pub(crate) fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) {
        if order.is_empty() || current_index >= order.len() {
            return;
        }

        self.update_node_positions(order, current_index);
        let free_layer = order[current_index].clone();
        let side = if forward {
            PortSide::West
        } else {
            PortSide::East
        };

        if !is_first_layer(order, current_index, forward) {
            let fixed_layer_index = if forward {
                current_index - 1
            } else {
                current_index + 1
            };
            let fixed_layer = order[fixed_layer_index].clone();

            self.calculate_port_ranks(graph, &fixed_layer, port_type_for(forward));
            for node in &free_layer {
                self.distribute_ports(graph, *node, side, order);
            }

            self.calculate_port_ranks(graph, &free_layer, port_type_for(!forward));
            for node in &fixed_layer {
                if graph.layerless_nodes[*node].nested_graph.is_none() {
                    self.distribute_ports(graph, *node, side.opposed(), order);
                }
            }
        } else {
            for node in &free_layer {
                self.distribute_ports(graph, *node, side, order);
            }
        }
    }

    pub(crate) fn calculate_port_ranks(
        &mut self,
        graph: &LGraph,
        layer: &[NodeId],
        port_type: PortType,
    ) {
        let mut consumed_rank = 0.0;
        for node in layer {
            consumed_rank += match self.kind {
                PortDistributorKind::NodeRelative => {
                    self.calculate_node_relative_port_ranks(graph, *node, consumed_rank, port_type)
                }
                PortDistributorKind::LayerTotal => {
                    self.calculate_layer_total_port_ranks(graph, *node, consumed_rank, port_type)
                }
            };
        }
    }

    pub(crate) fn rank_of(&self, graph: &LGraph, port: PortRef) -> f64 {
        crossing_minimization_port_id(graph, port)
            .and_then(|id| self.port_ranks.get(&id).copied())
            .unwrap_or(0.0)
    }

    pub(crate) fn distribute_ports(
        &mut self,
        graph: &mut LGraph,
        node: NodeId,
        side: PortSide,
        order: &[Vec<NodeId>],
    ) {
        if graph.layerless_nodes[node]
            .port_constraints
            .is_order_fixed()
        {
            return;
        }

        for port_side in [side, PortSide::South, PortSide::North] {
            let ports = graph.ports_on_side(node, port_side);
            self.distribute_ports_on_side(graph, node, &ports, order);
        }
        self.sort_ports(graph, node);
    }

    fn distribute_ports_on_side(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        ports: &[PortRef],
        order: &[Vec<NodeId>],
    ) {
        let mut in_layer_ports = Vec::new();
        self.min_barycenter = 0.0;
        self.max_barycenter = 0.0;
        let absurdly_large = 2.0 * layer_node_count(graph, node, order) as f64 + 1.0;

        'port_loop: for port in ports.iter().copied() {
            let Some(port_data) = graph
                .layerless_nodes
                .get(port.node)
                .and_then(|node| node.ports.get(port.port))
            else {
                continue;
            };

            let north_south_port = matches!(port_data.side, PortSide::North | PortSide::South);
            let mut sum = 0.0;
            if north_south_port {
                let Some(dummy_node) = port_data
                    .port_dummy
                    .as_ref()
                    .filter(|dummy| dummy.graph_id == graph.id)
                    .map(|dummy| dummy.node)
                else {
                    continue;
                };
                sum += north_south_port_barycenter_sum(
                    graph,
                    &self.node_positions,
                    port,
                    dummy_node,
                    absurdly_large,
                )
                .unwrap_or(0.0);
            } else {
                for outgoing_edge in port_data.outgoing_edges() {
                    let connected_port = graph.edges[*outgoing_edge].target();
                    if same_layer(graph, connected_port.node, node) {
                        in_layer_ports.push(port);
                        continue 'port_loop;
                    }
                    sum += self.rank_of(graph, connected_port);
                }
                for incoming_edge in port_data.incoming_edges() {
                    let connected_port = graph.edges[*incoming_edge].source();
                    if same_layer(graph, connected_port.node, node) {
                        in_layer_ports.push(port);
                        continue 'port_loop;
                    }
                    sum -= self.rank_of(graph, connected_port);
                }
            }

            let degree = port_degree(graph, port);
            if degree > 0 {
                let barycenter = sum / degree as f64;
                self.set_port_barycenter(graph, port, barycenter);
                self.min_barycenter = self.min_barycenter.min(barycenter);
                self.max_barycenter = self.max_barycenter.max(barycenter);
            } else if north_south_port {
                self.set_port_barycenter(graph, port, sum);
            }
        }

        if !in_layer_ports.is_empty() {
            self.calculate_in_layer_ports_barycenter_values(graph, node, &in_layer_ports, order);
        }
    }

    fn calculate_in_layer_ports_barycenter_values(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        in_layer_ports: &[PortRef],
        order: &[Vec<NodeId>],
    ) {
        let Some(node_position) = self.node_positions.get(&node).copied() else {
            return;
        };
        let node_index_in_layer = node_position + 1;
        let layer_size = layer_node_count(graph, node, order) + 1;

        for in_layer_port in in_layer_ports {
            let mut sum = 0usize;
            let mut in_layer_connections = 0usize;

            for connected_port in connected_ports(graph, *in_layer_port) {
                if same_layer(graph, connected_port.node, node)
                    && let Some(position) = self.node_positions.get(&connected_port.node)
                {
                    sum += position + 1;
                    in_layer_connections += 1;
                }
            }

            if in_layer_connections == 0 {
                continue;
            }

            let barycenter = sum as f64 / in_layer_connections as f64;
            let port_side =
                graph.layerless_nodes[in_layer_port.node].ports[in_layer_port.port].side;
            let port_barycenter = match port_side {
                PortSide::East if barycenter < node_index_in_layer as f64 => {
                    self.min_barycenter - barycenter
                }
                PortSide::East => self.max_barycenter + (layer_size as f64 - barycenter),
                PortSide::West if barycenter < node_index_in_layer as f64 => {
                    self.max_barycenter + barycenter
                }
                PortSide::West => self.min_barycenter - (layer_size as f64 - barycenter),
                _ => continue,
            };
            self.set_port_barycenter(graph, *in_layer_port, port_barycenter);
        }
    }

    fn sort_ports(&self, graph: &mut LGraph, node: NodeId) {
        let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        order.sort_by(|left, right| {
            let left_port = &graph.layerless_nodes[node].ports[*left];
            let right_port = &graph.layerless_nodes[node].ports[*right];

            let side_order = port_side_order(left_port.side).cmp(&port_side_order(right_port.side));
            if side_order != std::cmp::Ordering::Equal {
                return side_order;
            }

            let left_barycenter = self
                .port_barycenters
                .get(&left_port.crossing_minimization_id.unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(0.0);
            let right_barycenter = self
                .port_barycenters
                .get(&right_port.crossing_minimization_id.unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(0.0);

            if left_barycenter == 0.0 && right_barycenter == 0.0 {
                std::cmp::Ordering::Equal
            } else if left_barycenter == 0.0 {
                std::cmp::Ordering::Less
            } else if right_barycenter == 0.0 {
                std::cmp::Ordering::Greater
            } else {
                left_barycenter.total_cmp(&right_barycenter)
            }
        });
        // A sort that moves nothing is the common case, so skip the write.
        if order
            .iter()
            .copied()
            .eq(graph.layerless_nodes[node].ports.ids())
        {
            return;
        }
        graph.reorder_node_ports(node, order);
    }

    fn set_port_barycenter(&mut self, graph: &LGraph, port: PortRef, barycenter: f64) {
        if let Some(id) = crossing_minimization_port_id(graph, port) {
            self.port_barycenters.insert(id, barycenter);
        }
    }

    fn update_node_positions(&mut self, order: &[Vec<NodeId>], current_index: usize) {
        if let Some(layer) = order.get(current_index) {
            for (position, node) in layer.iter().copied().enumerate() {
                self.node_positions.insert(node, position);
            }
        }
    }

    fn calculate_node_relative_port_ranks(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        rank_sum: f64,
        port_type: PortType,
    ) -> f64 {
        match port_type {
            PortType::Input => {
                let input_count = actual_ports_of_type(graph, node, PortType::Input).len();
                let north_input_count = actual_ports_of_type(graph, node, PortType::Input)
                    .into_iter()
                    .filter(|port| {
                        graph.layerless_nodes[port.node].ports[port.port]
                            .side
                            .is_north()
                    })
                    .count();

                let increment = 1.0 / (input_count + 1) as f64;
                let mut north_position = rank_sum + north_input_count as f64 * increment;
                let mut rest_position = rank_sum + 1.0 - increment;
                for port in actual_ports_of_type(graph, node, PortType::Input) {
                    if graph.layerless_nodes[port.node].ports[port.port]
                        .side
                        .is_north()
                    {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, north_position);
                        }
                        north_position -= increment;
                    } else {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, rest_position);
                        }
                        rest_position -= increment;
                    }
                }
                1.0
            }
            PortType::Output => {
                let output_count = actual_ports_of_type(graph, node, PortType::Output).len();
                let increment = 1.0 / (output_count + 1) as f64;
                let mut position = rank_sum + increment;
                for port in actual_ports_of_type(graph, node, PortType::Output) {
                    if let Some(id) = crossing_minimization_port_id(graph, port) {
                        self.port_ranks.insert(id, position);
                    }
                    position += increment;
                }
                1.0
            }
        }
    }

    fn calculate_layer_total_port_ranks(
        &mut self,
        graph: &LGraph,
        node: NodeId,
        rank_sum: f64,
        port_type: PortType,
    ) -> f64 {
        match port_type {
            PortType::Input => {
                let input_ports = actual_ports_of_type(graph, node, PortType::Input);
                let input_count = input_ports.len();
                let north_input_count = input_ports
                    .iter()
                    .filter(|port| {
                        graph.layerless_nodes[port.node].ports[port.port]
                            .side
                            .is_north()
                    })
                    .count();

                let mut north_position = rank_sum + north_input_count as f64;
                let mut rest_position = rank_sum + input_count as f64;
                for port in input_ports {
                    if graph.layerless_nodes[port.node].ports[port.port]
                        .side
                        .is_north()
                    {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, north_position);
                        }
                        north_position -= 1.0;
                    } else {
                        if let Some(id) = crossing_minimization_port_id(graph, port) {
                            self.port_ranks.insert(id, rest_position);
                        }
                        rest_position -= 1.0;
                    }
                }
                input_count as f64
            }
            PortType::Output => {
                let mut position = 0.0;
                for port in actual_ports_of_type(graph, node, PortType::Output) {
                    position += 1.0;
                    if let Some(id) = crossing_minimization_port_id(graph, port) {
                        self.port_ranks.insert(id, rank_sum + position);
                    }
                }
                position
            }
        }
    }
}

fn actual_ports_of_type(graph: &LGraph, node: NodeId, port_type: PortType) -> Vec<PortRef> {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter_map(|(port, port_data)| {
            let matches = match port_type {
                PortType::Input => !port_data.incoming_edges().is_empty(),
                PortType::Output => !port_data.outgoing_edges().is_empty(),
            };
            matches.then_some(PortRef { node, port })
        })
        .collect()
}

fn crossing_minimization_port_id(graph: &LGraph, port: PortRef) -> Option<usize> {
    graph
        .layerless_nodes
        .get(port.node)?
        .ports
        .get(port.port)
        .and_then(|port_data| port_data.crossing_minimization_id)
}

fn connected_ports(graph: &LGraph, port: PortRef) -> Vec<PortRef> {
    connected_edges(graph, port)
        .filter_map(|edge| other_end_of_edge(graph, edge, port))
        .collect()
}

fn same_layer(graph: &LGraph, left: NodeId, right: NodeId) -> bool {
    graph.layerless_nodes[left].layer_index() == graph.layerless_nodes[right].layer_index()
}

pub(crate) fn north_south_port_barycenter_sum(
    graph: &LGraph,
    node_positions: &HashMap<NodeId, usize>,
    port: PortRef,
    dummy_node: NodeId,
    absurdly_large: f64,
) -> Option<f64> {
    let dummy = graph.layerless_nodes.get(dummy_node)?;
    let mut input = false;
    let mut output = false;

    for dummy_port in &dummy.ports {
        if dummy_port.node() != dummy_node {
            continue;
        }
        if !dummy_port.outgoing_edges().is_empty() {
            output = true;
        } else if !dummy_port.incoming_edges().is_empty() {
            input = true;
        }
    }

    let position = node_positions.get(&dummy_node).copied()? as f64;
    let side = graph.layerless_nodes[port.node].ports[port.port].side;

    let sum = if input && !output {
        if side == PortSide::North {
            -position
        } else {
            absurdly_large - position
        }
    } else if output && !input {
        position + 1.0
    } else if input && output {
        if side == PortSide::North {
            0.0
        } else {
            absurdly_large / 2.0
        }
    } else {
        0.0
    };
    Some(sum)
}

fn layer_node_count(graph: &LGraph, node: NodeId, order: &[Vec<NodeId>]) -> usize {
    let Some(layer_index) = graph.layerless_nodes[node].layer_index() else {
        return 0;
    };
    order.get(layer_index).map_or(0, Vec::len)
}

fn port_type_for(forward: bool) -> PortType {
    if forward {
        PortType::Output
    } else {
        PortType::Input
    }
}

fn port_side_order(side: PortSide) -> u8 {
    match side {
        PortSide::Undefined => 0,
        PortSide::North => 1,
        PortSide::East => 2,
        PortSide::South => 3,
        PortSide::West => 4,
    }
}

trait PortSideExt {
    fn is_north(&self) -> bool;
}

impl PortSideExt for crate::graph::PortSide {
    fn is_north(&self) -> bool {
        *self == crate::graph::PortSide::North
    }
}
