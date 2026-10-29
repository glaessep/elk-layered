//! Port of ELK's `GreedyPortDistributor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/GreedyPortDistributor.java

use crate::graph::{LGraph, NodeId, OtherGraphs, PortRef, PortSide};
use crate::intermediate::greedyswitch::between_layer_edge_two_node_crossings_counter::BetweenLayerEdgeTwoNodeCrossingsCounter;
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;

/// Ports `GreedyPortDistributor`: puts the ports of each node on the side that faces the sweep in
/// the order that has the fewest crossings, switching neighbours one pair at a time.
///
/// ELK uses it for the two-sided greedy switch. The other crossing minimizers use a barycenter
/// port distributor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GreedyPortDistributor {
    /// The counter keeps the port positions of every count it has made, as ELK's counter does:
    /// it owns an array that lives as long as the graph's `GraphInfoHolder`.
    pub(crate) crossings_counter: CrossingsCounter,
}

impl GreedyPortDistributor {
    /// `GreedyPortDistributor.distributePortsWhileSweeping`. `others` holds the graphs inside the
    /// nodes of `graph`, whose layers count in the crossings at a node's ports.
    pub(crate) fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        if current_index >= order.len() {
            return false;
        }
        self.initialize(graph, order, current_index, forward);
        self.distribute_ports_in_layer(graph, others, order, current_index, forward)
    }

    /// `GreedyPortDistributor.initialize`: numbers the ports the counts will compare.
    fn initialize(
        &mut self,
        graph: &LGraph,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) {
        if forward && current_index > 0 {
            self.crossings_counter.init_for_counting_between(
                graph,
                &order[current_index - 1],
                &order[current_index],
            );
        } else if !forward && current_index + 1 < order.len() {
            self.crossings_counter.init_for_counting_between(
                graph,
                &order[current_index],
                &order[current_index + 1],
            );
        } else {
            self.crossings_counter
                .init_port_positions_for_in_layer_crossings_with_tree(
                    graph,
                    &order[current_index],
                    if forward {
                        PortSide::West
                    } else {
                        PortSide::East
                    },
                );
        }
    }

    /// `GreedyPortDistributor.distributePortsInLayer`.
    fn distribute_ports_in_layer(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        let side = if forward {
            PortSide::West
        } else {
            PortSide::East
        };
        let mut improved = false;
        for &node in &order[current_index] {
            if graph.layerless_nodes[node]
                .port_constraints
                .is_order_fixed()
            {
                continue;
            }
            improved |= self.distribute_ports_on_node(graph, others, node, side, forward);
        }
        improved
    }

    /// `GreedyPortDistributor.distributePortsOnNode`.
    fn distribute_ports_on_node(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        node: NodeId,
        side: PortSide,
        forward: bool,
    ) -> bool {
        // The ports of the side in the order of the node's port list. A southern or western
        // side is read from the other end.
        let current_order = graph.layerless_nodes[node].ports.ids().collect::<Vec<_>>();
        let slots = current_order
            .iter()
            .enumerate()
            .filter(|(_, port)| graph.layerless_nodes[node].ports[**port].side == side)
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        let mut ports = slots
            .iter()
            .map(|slot| current_order[*slot])
            .collect::<Vec<_>>();
        if matches!(side, PortSide::South | PortSide::West) {
            ports.reverse();
        }

        // A node with children counts the crossings of its ports inside the node as well.
        let nested_graph = if ports.is_empty() {
            None
        } else {
            graph.layerless_nodes[node]
                .nested_graph
                .and_then(|nested| others.get(nested))
        };
        let inner_layers = nested_graph
            .map(|nested| nested.layers())
            .filter(|layers| !layers.is_empty());
        let mut hierarchical_counter = BetweenLayerEdgeTwoNodeCrossingsCounter::new();
        let inner_free_layer = inner_layers
            .as_ref()
            .map(|layers| if forward { 0 } else { layers.len() - 1 });

        let mut improved = false;
        loop {
            let mut continue_switching = false;
            for index in 0..ports.len().saturating_sub(1) {
                let upper_port = PortRef {
                    node,
                    port: ports[index],
                };
                let lower_port = PortRef {
                    node,
                    port: ports[index + 1],
                };
                let (mut upper_lower, mut lower_upper) = self
                    .crossings_counter
                    .count_crossings_between_ports_in_both_orders(graph, upper_port, lower_port);
                if let (Some(nested), Some(layers), Some(free_layer)) =
                    (nested_graph, inner_layers.as_ref(), inner_free_layer)
                {
                    let dummy_of = |port: PortRef| {
                        graph.layerless_nodes[node].ports[port.port]
                            .port_dummy
                            .as_ref()
                            .filter(|dummy| dummy.graph_id == nested.id)
                            .map(|dummy| dummy.node)
                    };
                    if let (Some(upper_node), Some(lower_node)) =
                        (dummy_of(upper_port), dummy_of(lower_port))
                    {
                        hierarchical_counter.count_both_side_crossings(
                            nested, layers, free_layer, upper_node, lower_node,
                        );
                        upper_lower += hierarchical_counter.upper_lower_crossings as f64;
                        lower_upper += hierarchical_counter.lower_upper_crossings as f64;
                    }
                }
                if upper_lower > lower_upper {
                    improved = true;
                    // The counter learns of the switch. The port list changes below, once.
                    self.crossings_counter
                        .switch_ports(graph, upper_port, lower_port);
                    ports.swap(index, index + 1);
                    continue_switching = true;
                }
            }
            if !continue_switching {
                break;
            }
        }

        if improved {
            // Write the new order of the side back to the node's port list.
            if matches!(side, PortSide::South | PortSide::West) {
                ports.reverse();
            }
            let mut new_order = current_order;
            for (slot, port) in slots.iter().zip(&ports) {
                new_order[*slot] = *port;
            }
            // The counter keys its positions by port id, which stays when a port moves. The order
            // is a permutation by construction, so a refusal is a bug in this function, never
            // caller input. It stays loud in release builds.
            assert!(
                graph.reorder_node_ports(node, new_order),
                "the switched port order of node {} is not a permutation of its ports",
                node.index()
            );
        }
        improved
    }
}
