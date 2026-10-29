//! Port of ELK's `BetweenLayerEdgeTwoNodeCrossingsCounter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/greedyswitch/BetweenLayerEdgeTwoNodeCrossingsCounter.java

use std::collections::HashMap;

use crate::graph::{EdgeId, LGraph, NodeId, PortRef, PortSide};
use crate::phase::ordering::counting::crossings_counter::ports_in_north_south_east_west_order;
use crate::phase::ordering::{connected_edges, is_edge_self_loop, is_in_layer_edge};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Adjacency {
    pub(crate) position: usize,
    pub(crate) cardinality: usize,
    current_cardinality: usize,
}

impl Adjacency {
    pub(crate) fn new(position: usize) -> Self {
        Self {
            position,
            cardinality: 1,
            current_cardinality: 1,
        }
    }

    fn reset(&mut self) {
        self.current_cardinality = self.cardinality;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AdjacencyList {
    pub(crate) adjacency_list: Vec<Adjacency>,
    pub(crate) size: usize,
    pub(crate) current_size: usize,
    current_index: usize,
}

impl AdjacencyList {
    /// `BetweenLayerEdgeTwoNodeCrossingsCounter.AdjacencyList`: its constructor and
    /// `getAdjacenciesSortedByPosition`, with `iterateTroughEdgesCollectingAdjacencies`. It
    /// collects the edges of `node` on `side`, port by port, in ELK's order, and then sorts them.
    fn new<L: AsRef<[NodeId]>>(
        graph: &LGraph,
        current_node_order: &[L],
        node: NodeId,
        side: PortSide,
    ) -> Self {
        let port_positions =
            neighboring_layer_port_positions(graph, current_node_order, node, side);
        let mut positions = Vec::new();
        for port in ports_in_north_south_east_west_order(graph, node, side) {
            for edge in edges_connected_to_side(graph, port, side) {
                if !is_edge_self_loop(graph, edge) && !is_in_layer_edge(graph, edge) {
                    let adjacent_port = adjacent_port_of_side(graph, edge, side);
                    // An edge that leaves the free layer and is not an in-layer edge ends in the
                    // neighbouring layer, so its port has a position: the layering is proper.
                    // ELK reads the position from a HashMap<LPort, Integer> and unboxes it into an
                    // int, so a missing port would throw. The skip cannot happen in a proper
                    // layering; it only keeps a broken one from panicking.
                    if let Some(&position) = port_positions.get(&adjacent_port) {
                        positions.push(position);
                    }
                }
            }
        }
        Self::from_positions(positions)
    }

    /// The list of the positions an edge reaches, in the order the edges were collected.
    pub(crate) fn from_positions(positions: impl IntoIterator<Item = usize>) -> Self {
        let mut adjacency_list = Vec::new();
        let mut size = 0usize;
        for position in positions {
            add_adjacency(&mut adjacency_list, position);
            size += 1;
        }
        // Collections.sort is stable, and so is sort_by_key. Two entries at one position that the
        // collection did not merge stay two entries, in the order they were collected.
        adjacency_list.sort_by_key(|adjacency| adjacency.position);
        Self {
            adjacency_list,
            size,
            current_size: size,
            current_index: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.current_index = 0;
        self.current_size = self.size;
        if !self.is_empty() {
            self.current_adjacency_mut().reset();
        }
    }

    pub(crate) fn count_adjacencies_below_node_of_first_port(&self) -> usize {
        self.current_size - self.current_adjacency().current_cardinality
    }

    fn remove_first(&mut self) {
        if self.is_empty() {
            return;
        }

        if self.current_adjacency().current_cardinality == 1 {
            self.increment_current_index();
        } else {
            self.current_adjacency_mut().current_cardinality -= 1;
        }

        self.current_size -= 1;
    }

    fn increment_current_index(&mut self) {
        self.current_index += 1;
        if self.current_index < self.adjacency_list.len() {
            self.current_adjacency_mut().reset();
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.current_size == 0
    }

    fn first(&self) -> usize {
        self.current_adjacency().position
    }

    fn current_size(&self) -> usize {
        self.current_size
    }

    fn current_adjacency(&self) -> &Adjacency {
        &self.adjacency_list[self.current_index]
    }

    fn current_adjacency_mut(&mut self) -> &mut Adjacency {
        &mut self.adjacency_list[self.current_index]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BetweenLayerEdgeTwoNodeCrossingsCounter {
    pub(crate) upper_lower_crossings: usize,
    pub(crate) lower_upper_crossings: usize,
    eastern_adjacencies: HashMap<NodeId, AdjacencyList>,
    western_adjacencies: HashMap<NodeId, AdjacencyList>,
}

impl BetweenLayerEdgeTwoNodeCrossingsCounter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn count_eastern_edge_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::East,
        );
    }

    pub(crate) fn count_western_edge_crossings(
        &mut self,
        graph: &LGraph,
        current_node_order: &[Vec<NodeId>],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::West,
        );
    }

    pub(crate) fn count_both_side_crossings<L: AsRef<[NodeId]>>(
        &mut self,
        graph: &LGraph,
        current_node_order: &[L],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
    ) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::West,
        );
        self.add_crossings(
            graph,
            current_node_order,
            free_layer_index,
            upper_node,
            lower_node,
            PortSide::East,
        );
    }

    fn reset_crossing_count(&mut self) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;
    }

    fn add_crossings<L: AsRef<[NodeId]>>(
        &mut self,
        graph: &LGraph,
        current_node_order: &[L],
        free_layer_index: usize,
        upper_node: NodeId,
        lower_node: NodeId,
        side: PortSide,
    ) {
        if !neighboring_layer_exists(current_node_order, free_layer_index, side) {
            return;
        }

        let (upper, lower) = match side {
            PortSide::East => {
                if self.eastern_adjacencies.is_empty() {
                    self.eastern_adjacencies =
                        build_adjacencies(graph, current_node_order, free_layer_index, side);
                }
                (
                    self.eastern_adjacencies.get(&upper_node).cloned(),
                    self.eastern_adjacencies.get(&lower_node).cloned(),
                )
            }
            PortSide::West => {
                if self.western_adjacencies.is_empty() {
                    self.western_adjacencies =
                        build_adjacencies(graph, current_node_order, free_layer_index, side);
                }
                (
                    self.western_adjacencies.get(&upper_node).cloned(),
                    self.western_adjacencies.get(&lower_node).cloned(),
                )
            }
            _ => (None, None),
        };

        let (Some(mut upper), Some(mut lower)) = (upper, lower) else {
            return;
        };
        upper.reset();
        lower.reset();
        if upper.current_size() == 0 || lower.current_size() == 0 {
            return;
        }
        self.count_crossings_by_merging_adjacency_lists(upper, lower);
    }

    pub(crate) fn count_crossings_by_merging_adjacency_lists(
        &mut self,
        mut upper_adjacencies: AdjacencyList,
        mut lower_adjacencies: AdjacencyList,
    ) {
        while !upper_adjacencies.is_empty() && !lower_adjacencies.is_empty() {
            if is_below(upper_adjacencies.first(), lower_adjacencies.first()) {
                self.upper_lower_crossings += upper_adjacencies.current_size();
                lower_adjacencies.remove_first();
            } else if is_below(lower_adjacencies.first(), upper_adjacencies.first()) {
                self.lower_upper_crossings += lower_adjacencies.current_size();
                upper_adjacencies.remove_first();
            } else {
                self.upper_lower_crossings +=
                    upper_adjacencies.count_adjacencies_below_node_of_first_port();
                self.lower_upper_crossings +=
                    lower_adjacencies.count_adjacencies_below_node_of_first_port();
                upper_adjacencies.remove_first();
                lower_adjacencies.remove_first();
            }
        }
    }
}

fn build_adjacencies<L: AsRef<[NodeId]>>(
    graph: &LGraph,
    current_node_order: &[L],
    free_layer_index: usize,
    side: PortSide,
) -> HashMap<NodeId, AdjacencyList> {
    current_node_order
        .get(free_layer_index)
        .into_iter()
        .flat_map(|layer| layer.as_ref().iter())
        .copied()
        .map(|node| {
            (
                node,
                AdjacencyList::new(graph, current_node_order, node, side),
            )
        })
        .collect()
}

/// `BetweenLayerEdgeTwoNodeCrossingsCounter.AdjacencyList.addAdjacencyOf`. An edge to the
/// position of the entry added last raises that entry's cardinality; any other edge adds a new
/// entry. So two edges to one port merge only when they are collected one after the other. ELK
/// sorts the list afterwards and never merges the entries that the sort brings together. The
/// count then takes the second entry at a position as an adjacency below the first, and a switch
/// can look better than it is. elkjs counts this way, so this port does too.
pub(crate) fn add_adjacency(adjacency_list: &mut Vec<Adjacency>, position: usize) {
    if let Some(last) = adjacency_list.last_mut()
        && last.position == position
    {
        last.cardinality += 1;
        last.current_cardinality += 1;
        return;
    }
    adjacency_list.push(Adjacency::new(position));
}

fn neighboring_layer_port_positions<L: AsRef<[NodeId]>>(
    graph: &LGraph,
    current_node_order: &[L],
    node: NodeId,
    side: PortSide,
) -> HashMap<PortRef, usize> {
    let mut positions = HashMap::new();
    let Some(layer_index) = graph.layerless_nodes[node].layer_index() else {
        return positions;
    };
    let neighbor = match side {
        PortSide::West if layer_index > 0 => Some((layer_index - 1, PortSide::East)),
        PortSide::East if layer_index + 1 < current_node_order.len() => {
            Some((layer_index + 1, PortSide::West))
        }
        _ => None,
    };
    let Some((neighbor_index, neighbor_side)) = neighbor else {
        return positions;
    };

    for (position, port) in current_node_order[neighbor_index]
        .as_ref()
        .iter()
        .copied()
        .flat_map(|node| ports_in_north_south_east_west_order(graph, node, neighbor_side))
        .enumerate()
    {
        positions.insert(port, position);
    }
    positions
}

fn neighboring_layer_exists<L: AsRef<[NodeId]>>(
    current_node_order: &[L],
    free_layer_index: usize,
    side: PortSide,
) -> bool {
    match side {
        PortSide::West => free_layer_index > 0,
        PortSide::East => free_layer_index + 1 < current_node_order.len(),
        _ => false,
    }
}

fn edges_connected_to_side(graph: &LGraph, port: PortRef, side: PortSide) -> Vec<EdgeId> {
    match side {
        PortSide::West => graph.layerless_nodes[port.node].ports[port.port]
            .incoming_edges()
            .to_vec(),
        PortSide::East => graph.layerless_nodes[port.node].ports[port.port]
            .outgoing_edges()
            .to_vec(),
        _ => connected_edges(graph, port).collect(),
    }
}

fn adjacent_port_of_side(graph: &LGraph, edge: EdgeId, side: PortSide) -> PortRef {
    if side == PortSide::West {
        graph.edges[edge].source()
    } else {
        graph.edges[edge].target()
    }
}

fn is_below(first_port: usize, second_port: usize) -> bool {
    first_port > second_port
}
