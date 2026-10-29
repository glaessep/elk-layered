//! Port of ELK's `AllCrossingsCounter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/AllCrossingsCounter.java

use crate::graph::{LGraph, LNodeKind, NodeId, PortSide};
use crate::phase::ordering::counting::binary_indexed_tree::BinaryIndexedTree;
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;

/// `AllCrossingsCounter.initAtNodeLevel`: `hasNorthSouthPorts[l]` is set when layer `l` has a
/// north or south port dummy.
pub(crate) fn has_north_south_ports(graph: &LGraph, order: &[Vec<NodeId>]) -> Vec<bool> {
    order
        .iter()
        .map(|layer| {
            layer
                .iter()
                .any(|&node| graph.layerless_nodes[node].kind() == LNodeKind::NorthSouthPort)
        })
        .collect()
}

/// `AllCrossingsCounter.initAtPortLevel`: `hasHyperEdgesEastOfIndex[l]` is set when an east port of
/// layer `l`, or a west port of layer `l + 1`, has more than one edge. Only real ports can have
/// that; a port ELK makes up for a plain edge has exactly one.
pub(crate) fn has_hyperedges_east_of_index(graph: &LGraph, order: &[Vec<NodeId>]) -> Vec<bool> {
    let mut flags = vec![false; order.len()];
    for (layer_index, layer) in order.iter().enumerate() {
        for node in layer {
            for port in &graph.layerless_nodes[*node].ports {
                if port.outgoing_edges().len() + port.incoming_edges().len() <= 1 {
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

impl CrossingsCounter {
    pub fn count_crossings_between_layers(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[NodeId],
        right_layer_nodes: &[NodeId],
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
        crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
            &mut graph,
        );
        let graph = &graph;
        let current_order = graph
            .layers()
            .iter()
            .map(|layer| layer.nodes().to_vec())
            .collect::<Vec<_>>();
        self.count_all_crossings_in_order(graph, &current_order)
    }

    pub fn count_all_crossings_in_order(
        &mut self,
        graph: &LGraph,
        current_order: &[Vec<NodeId>],
    ) -> f64 {
        if current_order.is_empty() {
            return 0.0;
        }

        // Taken out of `self` for the loop, which counts with `&mut self`, and put back after it.
        let has_hyperedges_east_of_index = self
            .has_hyperedges_east_of_index
            .take()
            .unwrap_or_else(|| has_hyperedges_east_of_index(graph, current_order));
        let has_north_south_ports = self
            .has_north_south_ports
            .take()
            .unwrap_or_else(|| has_north_south_ports(graph, current_order));
        let mut crossings =
            self.count_in_layer_crossings_on_side(graph, &current_order[0], PortSide::West);
        crossings += self.count_in_layer_crossings_on_side(
            graph,
            &current_order[current_order.len() - 1],
            PortSide::East,
        );
        for layer_index in 0..current_order.len() {
            // AllCrossingsCounter.countCrossingsAt: a layer pair with a port that has more than
            // one edge goes to the hyperedge counter, which also needs the in-layer counts.
            let left_layer = &current_order[layer_index];
            if let Some(right_layer) = current_order.get(layer_index + 1) {
                if has_hyperedges_east_of_index[layer_index] {
                    crossings +=
                        self.count_hyperedge_crossings(graph, left_layer, right_layer) as f64;
                    crossings +=
                        self.count_in_layer_crossings_on_side(graph, left_layer, PortSide::East);
                    crossings +=
                        self.count_in_layer_crossings_on_side(graph, right_layer, PortSide::West);
                } else {
                    crossings +=
                        self.count_crossings_between_layers(graph, left_layer, right_layer);
                }
            }

            // The north and south port dummies of the layer cross the edges that pass it.
            if has_north_south_ports[layer_index] {
                crossings += self.count_north_south_port_crossings_in_layer(graph, left_layer);
            }
        }
        self.has_hyperedges_east_of_index = Some(has_hyperedges_east_of_index);
        self.has_north_south_ports = Some(has_north_south_ports);
        crossings
    }
}
