//! Port of ELK's `NorthSouthEdgeNeighbouringNodeCrossingsCounter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/NorthSouthEdgeNeighbouringNodeCrossingsCounter.java

use crate::graph::{LGraph, LNodeKind, NodeId, PortRef, PortSide};
use crate::phase::ordering::counting::crossings_counter::{
    first_port_on_side, north_south_ports_with_incident_edges, port_degree,
    ports_in_north_south_east_west_order,
};
use crate::phase::ordering::{is_long_edge_dummy, is_north_south_node};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    pub(crate) upper_lower_crossings: usize,
    pub(crate) lower_upper_crossings: usize,
    /// The place of each northern and southern port among the ports of its side and node, by
    /// `crossing_minimization_id` as in `CrossingsCounter`. A port nobody numbered is at 0.
    port_positions: Vec<usize>,
}

impl NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    /// Ports the constructor: it numbers the northern and southern ports of the layer's nodes.
    pub(crate) fn new(graph: &LGraph, layer: &[NodeId]) -> Self {
        let mut port_positions = Vec::new();
        for &node in layer {
            for side in [PortSide::South, PortSide::North] {
                for (position, port) in ports_in_north_south_east_west_order(graph, node, side)
                    .into_iter()
                    .enumerate()
                {
                    let Some(id) =
                        graph.layerless_nodes[port.node].ports[port.port].crossing_minimization_id
                    else {
                        continue;
                    };
                    if port_positions.len() <= id {
                        port_positions.resize(id + 1, 0);
                    }
                    port_positions[id] = position;
                }
            }
        }
        Self {
            port_positions,
            ..Self::default()
        }
    }

    pub(crate) fn count_crossings(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;

        self.process_if_two_north_south_nodes(graph, upper_node, lower_node);
        self.process_if_north_south_long_edge_dummy_crossing(graph, upper_node, lower_node);
        self.process_if_normal_node_with_ns_ports_and_long_edge_dummy(
            graph, upper_node, lower_node,
        );
    }

    /// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.processIfTwoNorthSouthNodes`.
    fn process_if_two_north_south_nodes(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if is_north_south_node(graph, upper_node)
            && is_north_south_node(graph, lower_node)
            && !have_different_origins(graph, upper_node, lower_node)
        {
            if north_south_dummy_is_north_of_normal_node(graph, upper_node) {
                self.count_crossings_of_two_north_south_dummies(graph, upper_node, lower_node);
            } else {
                self.count_crossings_of_two_north_south_dummies(graph, lower_node, upper_node);
            }
        }
    }

    /// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.countCrossingsOfTwoNorthSouthDummies`.
    fn count_crossings_of_two_north_south_dummies(
        &mut self,
        graph: &LGraph,
        further_from_normal_node: NodeId,
        closer_to_normal_node: NodeId,
    ) {
        let first_degree = |node: NodeId, side: PortSide| {
            first_port_on_side(graph, node, side).map_or(0, |port| port_degree(graph, port))
        };
        if self.origin_port_position_of(graph, further_from_normal_node)
            > self.origin_port_position_of(graph, closer_to_normal_node)
        {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::East);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::West);
        } else {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::West);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::East);
        }
    }

    fn process_if_north_south_long_edge_dummy_crossing(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if is_north_south_node(graph, upper_node) && is_long_edge_dummy(graph, lower_node) {
            if north_south_dummy_is_north_of_normal_node(graph, upper_node) {
                self.upper_lower_crossings = 1;
            } else {
                self.lower_upper_crossings = 1;
            }
        } else if is_north_south_node(graph, lower_node) && is_long_edge_dummy(graph, upper_node) {
            if north_south_dummy_is_north_of_normal_node(graph, lower_node) {
                self.lower_upper_crossings = 1;
            } else {
                self.upper_lower_crossings = 1;
            }
        }
    }

    fn process_if_normal_node_with_ns_ports_and_long_edge_dummy(
        &mut self,
        graph: &LGraph,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        if graph.layerless_nodes[upper_node].kind() == LNodeKind::Normal
            && is_long_edge_dummy(graph, lower_node)
        {
            self.upper_lower_crossings =
                number_of_north_south_edges(graph, upper_node, PortSide::South);
            self.lower_upper_crossings =
                number_of_north_south_edges(graph, upper_node, PortSide::North);
        }
        if graph.layerless_nodes[lower_node].kind() == LNodeKind::Normal
            && is_long_edge_dummy(graph, upper_node)
        {
            self.upper_lower_crossings =
                number_of_north_south_edges(graph, lower_node, PortSide::North);
            self.lower_upper_crossings =
                number_of_north_south_edges(graph, lower_node, PortSide::South);
        }
    }

    fn origin_port_position_of(&self, graph: &LGraph, node: NodeId) -> usize {
        // The origin port is a northern or southern port of a node in the layer, so it has a place.
        origin_port_of(graph, node)
            .and_then(|origin| {
                graph.layerless_nodes[origin.node].ports[origin.port].crossing_minimization_id
            })
            .and_then(|id| self.port_positions.get(id).copied())
            .unwrap_or(0)
    }
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.originPortOf`: the port of the regular node a
/// dummy was made for.
fn origin_port_of(graph: &LGraph, node: NodeId) -> Option<PortRef> {
    graph.layerless_nodes[node]
        .ports
        .first()
        .and_then(|port| port.origin_port)
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.isNorthOfNormalNode`.
fn north_south_dummy_is_north_of_normal_node(graph: &LGraph, node: NodeId) -> bool {
    origin_port_of(graph, node).is_some_and(|origin| {
        graph.layerless_nodes[origin.node].ports[origin.port].side == PortSide::North
    })
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.haveDifferentOrigins`: two dummies have the
/// origin of their ports in different nodes.
fn have_different_origins(graph: &LGraph, upper_node: NodeId, lower_node: NodeId) -> bool {
    origin_port_of(graph, upper_node).map(|origin| origin.node)
        != origin_port_of(graph, lower_node).map(|origin| origin.node)
}

/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.numberOfNorthSouthEdges`: the ports of the
/// side that have a dummy.
fn number_of_north_south_edges(graph: &LGraph, node: NodeId, side: PortSide) -> usize {
    north_south_ports_with_incident_edges(graph, node, side).count()
}
