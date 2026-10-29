//! Port of ELK's `SweepCopy`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/SweepCopy.java

use crate::graph::{IdVec, LGraph, LNodeKind, NodeId, PortId, PortSide};
use crate::intermediate::port_list_sorter::compare_combined_ports;
use crate::options::PortConstraints;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepCopy {
    pub node_order: Vec<Vec<NodeId>>,
    /// The ports of each node, in its port order, by layer and place in the layer. Java keeps
    /// the `LPort` objects here. A `PortId` is the same identity.
    pub port_orders: Vec<Vec<Vec<PortId>>>,
}

impl SweepCopy {
    pub fn new(graph: &LGraph, node_order: &[Vec<NodeId>]) -> Self {
        Self {
            node_order: node_order.to_vec(),
            port_orders: node_order
                .iter()
                .map(|layer| {
                    layer
                        .iter()
                        .map(|node| graph.layerless_nodes[*node].ports.ids().collect())
                        .collect()
                })
                .collect(),
        }
    }

    pub fn transfer_node_and_port_orders_to_graph(
        &self,
        graph: &mut LGraph,
        set_port_constraints: bool,
    ) -> bool {
        if self.node_order.len() != graph.layers().len() {
            return false;
        }

        for (layer_index, layer_order) in self.node_order.iter().enumerate() {
            if layer_order.len() != graph.layers()[layer_index].nodes().len() {
                return false;
            }
        }

        for (layer_index, layer_order) in self.node_order.iter().enumerate() {
            graph.set_layer_nodes(layer_index, layer_order.clone());
            for (position, node) in layer_order.iter().copied().enumerate() {
                let Some(port_order) = self
                    .port_orders
                    .get(layer_index)
                    .and_then(|layer| layer.get(position))
                else {
                    return false;
                };
                if !graph.reorder_node_ports(node, port_order.iter().copied()) {
                    return false;
                }
                if set_port_constraints
                    && !graph.layerless_nodes[node]
                        .port_constraints
                        .is_order_fixed()
                {
                    graph.layerless_nodes[node].port_constraints = PortConstraints::FixedOrder;
                }
            }
        }

        correct_north_south_port_sides(graph);

        true
    }
}

/// Ports the end of `SweepCopy.transferNodeAndPortOrdersToGraph` and `assertCorrectPortSides`.
///
/// The option `allowNonFlowPortsToSwitchSides` lets the crossing minimization place the dummy of
/// a port on either side of its node. A dummy below its node means the port is a southern one,
/// and a dummy above means a northern one, so the side of the port changes to match. The ports of
/// each dummy and of each node that has a dummy are sorted again, side by side.
pub(crate) fn correct_north_south_port_sides(graph: &mut LGraph) {
    let mut to_sort: Vec<NodeId> = Vec::new();
    // Which nodes are in `to_sort` already, and where a node sits in the layer being read.
    let mut queued = IdVec::filled(graph.layerless_nodes.len(), false);
    let mut position_in_layer = IdVec::filled(graph.layerless_nodes.len(), usize::MAX);
    for layer_index in 0..graph.layers().len() {
        let layer = graph.layers()[layer_index].nodes().to_vec();
        for (position, node) in layer.iter().copied().enumerate() {
            position_in_layer[node] = position;
        }
        for (position, dummy) in layer.iter().copied().enumerate() {
            if graph.layerless_nodes[dummy].kind() != LNodeKind::NorthSouthPort {
                continue;
            }
            // The node the dummy was made for is in its layout unit, in the same layer.
            let Some(origin) = graph.in_layer_layout_unit(dummy) else {
                continue;
            };
            // `position` and `origin_position` are the final places in the layer. Java compares
            // the same numbers: `transferNodeAndPortOrdersToGraph` sets `node.id = j` for every
            // node of the layer first, and only then `assertCorrectPortSides` compares
            // `dummy.id > origin.id`. The ids from before the sweep play no part.
            let origin_position = position_in_layer[origin];
            if origin_position == usize::MAX {
                continue;
            }
            let origin_port = graph.layerless_nodes[dummy]
                .ports
                .first()
                .and_then(|port| port.origin_port);
            if let Some(origin_port) = origin_port.filter(|port| port.node == origin) {
                let port = &mut graph.layerless_nodes[origin].ports[origin_port.port];
                if port.side == PortSide::North && position > origin_position {
                    port.set_side(PortSide::South);
                } else if port.side == PortSide::South && origin_position > position {
                    port.set_side(PortSide::North);
                }
            }
            for node in [origin, dummy] {
                if !queued[node] {
                    queued[node] = true;
                    to_sort.push(node);
                }
            }
        }
        for node in layer {
            position_in_layer[node] = usize::MAX;
        }
    }

    // The side of a port may have changed, so the list is sorted again, as `PortListSorter` sorts
    // a node with a fixed port order. Writing the list back costs one copy of the ids, and a
    // port keeps its id, so a sorted list needs no special case.
    for node in to_sort {
        let constraints = graph.layerless_nodes[node].port_constraints;
        let mut order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        order
            .sort_by(|left, right| compare_combined_ports(graph, node, *left, *right, constraints));
        // A sort of the port indices is a permutation, so a refusal is a bug, never caller
        // input. It stays loud in release builds.
        assert!(
            graph.reorder_node_ports(node, order),
            "the sorted port order of node {} is not a permutation of its ports",
            node.index()
        );
    }
}
