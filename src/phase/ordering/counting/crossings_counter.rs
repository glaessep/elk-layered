//! Port of ELK's `CrossingsCounter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/CrossingsCounter.java
//!
//! One struct, `CrossingsCounter`, holds the state that ELK's `AllCrossingsCounter`,
//! `CrossingsCounter` and `HyperedgeCrossingsCounter` share. Each of those classes has its own
//! file with its own `impl` block.

use crate::graph::{EdgeId, LGraph, LNodeKind, NodeId, PortRef, PortSide};
use crate::phase::ordering::counting::binary_indexed_tree::BinaryIndexedTree;

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
    /// `AllCrossingsCounter.hasHyperEdgesEastOfIndex`. ELK sets it once, in `initAtPortLevel`,
    /// when it builds the counter. The first count sets it here, and no later count reads the
    /// ports again: the edges at a port do not change during the sweep.
    pub(crate) has_hyperedges_east_of_index: Option<Vec<bool>>,
    /// `AllCrossingsCounter.hasNorthSouthPorts`, set in `initAtNodeLevel` when ELK builds the
    /// counter. The first count sets it here, like `has_hyperedges_east_of_index`.
    pub(crate) has_north_south_ports: Option<Vec<bool>>,
    /// The tree that `initForCountingBetween` and `initPortPositionsForInLayerCrossings` make for
    /// the counts of single port pairs.
    index_tree: Option<BinaryIndexedTree>,
}

impl CrossingsCounter {
    /// `CrossingsCounter.countNorthSouthPortCrossingsInLayer`: the crossings between the edges
    /// of the north and south ports of a layer's nodes, and between these edges and the long
    /// edges that pass the layer.
    ///
    /// A north or south port has no edge of its own at this point: its dummy node has them. The
    /// count joins each port to its dummy's ports as if an edge ran between them, and numbers the
    /// ports of the layer so these edges become in-layer edges.
    pub fn count_north_south_port_crossings_in_layer(
        &mut self,
        graph: &LGraph,
        layer: &[NodeId],
    ) -> f64 {
        let ports = self.init_positions_for_north_south_counting(graph, layer);
        let mut index_tree = BinaryIndexedTree::new(ports.len());
        self.count_north_south_crossings_on_ports(graph, &ports, &mut index_tree)
    }

    /// `CrossingsCounter.countNorthSouthCrossingsOnPorts`: the crossings among the in-layer
    /// edges that join the north and south ports of a layer to their dummies.
    fn count_north_south_crossings_on_ports(
        &self,
        graph: &LGraph,
        ports: &[PortRef],
        index_tree: &mut BinaryIndexedTree,
    ) -> f64 {
        let mut crossings = 0.0f64;
        let mut ends = Vec::new();
        // The ports an implicit edge of the current port ends at, with their degrees.
        let mut targets_and_degrees: Vec<(PortRef, usize)> = Vec::new();

        for &port in ports {
            let port_position = self.position_of(graph, port);
            index_tree.remove_all(port_position);
            targets_and_degrees.clear();

            // Collect the edges of the port, which is a bit tedious: north and south ports
            // have no physical edge in the graph at this point.
            let node = &graph.layerless_nodes[port.node];
            match node.kind() {
                LNodeKind::Normal => {
                    // The init step only numbers ports that have a dummy.
                    if let Some(dummy) = node.ports[port.port].port_dummy.as_ref()
                        && dummy.graph_id == graph.id
                    {
                        // Its western and eastern port.
                        for (index, _) in graph.layerless_nodes[dummy.node].ports.iter_with_ids() {
                            let target = PortRef {
                                node: dummy.node,
                                port: index,
                            };
                            targets_and_degrees.push((target, port_degree(graph, target)));
                        }
                    }
                }
                LNodeKind::LongEdge => {
                    // An edge to the other port of the dummy.
                    if let Some(index) = node.ports.ids().find(|&index| index != port.port) {
                        let target = PortRef {
                            node: port.node,
                            port: index,
                        };
                        targets_and_degrees.push((target, port_degree(graph, target)));
                    }
                }
                LNodeKind::NorthSouthPort => {
                    if let Some(origin) = node.ports[port.port].origin_port {
                        targets_and_degrees.push((origin, port_degree(graph, port)));
                    }
                }
                _ => {}
            }

            // First get the crossings for all edges.
            for &(target, target_degree) in &targets_and_degrees {
                let end_position = self.position_of(graph, target);
                if end_position > port_position {
                    crossings += rank_or_nan(index_tree.rank(end_position)) * target_degree as f64;
                    ends.push(end_position);
                }
            }

            // Then add the end points.
            while let Some(end) = ends.pop() {
                index_tree.add(end);
            }
        }

        crossings
    }

    /// `CrossingsCounter.initPositionsForNorthSouthCounting`. The top-down order of the layer is:
    /// the north dummies with western edges, the north ports from west to east, the north dummies
    /// with eastern edges, the south ports from east to west, then the south dummies with western
    /// edges and the south dummies with eastern edges.
    pub(crate) fn init_positions_for_north_south_counting(
        &mut self,
        graph: &LGraph,
        nodes: &[NodeId],
    ) -> Vec<PortRef> {
        // The side that gets an index in the order of the layer, and the side whose dummies wait
        // on a stack to get theirs later.
        const INDEXING_SIDE: PortSide = PortSide::West;
        const STACK_SIDE: PortSide = PortSide::East;

        let mut ports = Vec::new();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut last_layout_unit: Option<NodeId> = None;
        let mut index = 0usize;

        for &current in nodes {
            let node = &graph.layerless_nodes[current];
            let layout_unit = graph.in_layer_layout_unit(current);
            if is_layout_unit_changed(last_layout_unit, current, layout_unit) {
                // Work the stack, which is full of southern dummies.
                index = self.empty_stack(graph, &mut stack, &mut ports, STACK_SIDE, index);
            }
            if layout_unit.is_some() {
                last_layout_unit = layout_unit;
            }

            match node.kind() {
                LNodeKind::Normal => {
                    // The northern ports from west to east.
                    for port in
                        north_south_ports_with_incident_edges(graph, current, PortSide::North)
                    {
                        self.number_port(graph, &mut ports, port, &mut index);
                    }

                    // Work the stack, which is full of northern dummies.
                    index = self.empty_stack(graph, &mut stack, &mut ports, STACK_SIDE, index);

                    // The southern ports in the regular clockwise order.
                    for port in
                        north_south_ports_with_incident_edges(graph, current, PortSide::South)
                    {
                        self.number_port(graph, &mut ports, port, &mut index);
                    }
                }
                LNodeKind::NorthSouthPort => {
                    // There should be only one.
                    if let Some(port) = first_port_on_side(graph, current, INDEXING_SIDE) {
                        self.number_port(graph, &mut ports, port, &mut index);
                    }
                    if first_port_on_side(graph, current, STACK_SIDE).is_some() {
                        stack.push(current);
                    }
                }
                LNodeKind::LongEdge => {
                    for port in node
                        .ports
                        .iter_with_ids()
                        .filter(|(_, port)| port.side == PortSide::West)
                        .map(|(port, _)| PortRef {
                            node: current,
                            port,
                        })
                    {
                        self.number_port(graph, &mut ports, port, &mut index);
                    }
                    // A long edge dummy has one eastern port, so it goes on the stack once.
                    for _ in node.ports.iter().filter(|port| port.side == PortSide::East) {
                        stack.push(current);
                    }
                }
                _ => {}
            }
        }

        // Are there any southern dummy nodes left on the stack?
        self.empty_stack(graph, &mut stack, &mut ports, STACK_SIDE, index);
        ports
    }

    fn number_port(
        &mut self,
        graph: &LGraph,
        ports: &mut Vec<PortRef>,
        port: PortRef,
        index: &mut usize,
    ) {
        self.set_position(graph, port, *index);
        *index += 1;
        ports.push(port);
    }

    /// `CrossingsCounter.emptyStack`. Each dummy on the stack is a north or south port dummy or a
    /// long edge dummy, and has a single port on `side`.
    fn empty_stack(
        &mut self,
        graph: &LGraph,
        stack: &mut Vec<NodeId>,
        ports: &mut Vec<PortRef>,
        side: PortSide,
        start_index: usize,
    ) -> usize {
        let mut index = start_index;
        while let Some(dummy) = stack.pop() {
            if let Some(port) = first_port_on_side(graph, dummy, side) {
                self.number_port(graph, ports, port, &mut index);
            }
        }
        index
    }

    pub fn count_in_layer_crossings_on_side(
        &mut self,
        graph: &LGraph,
        nodes: &[NodeId],
        side: PortSide,
    ) -> f64 {
        let ports = self.init_port_positions_for_in_layer_crossings(graph, nodes, side);
        let mut index_tree = BinaryIndexedTree::new(ports.len());
        self.count_in_layer_crossings_on_ports(graph, &ports, &mut index_tree)
    }

    /// `CrossingsCounter.initForCountingBetween`: numbers the ports between two layers, for
    /// `count_crossings_between_ports_in_both_orders`.
    pub fn init_for_counting_between(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[NodeId],
        right_layer_nodes: &[NodeId],
    ) {
        let ports =
            self.init_port_positions_counter_clockwise(graph, left_layer_nodes, right_layer_nodes);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
    }

    /// `CrossingsCounter.initPortPositionsForInLayerCrossings`: numbers the ports of one side of a
    /// layer, for `count_crossings_between_ports_in_both_orders`.
    pub fn init_port_positions_for_in_layer_crossings_with_tree(
        &mut self,
        graph: &LGraph,
        nodes: &[NodeId],
        side: PortSide,
    ) {
        let ports = self.init_port_positions_for_in_layer_crossings(graph, nodes, side);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
    }

    /// `CrossingsCounter.switchPorts`: swaps the positions of two ports.
    pub fn switch_ports(&mut self, graph: &LGraph, top_port: PortRef, bottom_port: PortRef) {
        let top_position = self.position_of(graph, top_port);
        let bottom_position = self.position_of(graph, bottom_port);
        self.set_position(graph, top_port, bottom_position);
        self.set_position(graph, bottom_port, top_position);
    }

    /// `CrossingsCounter.countCrossingsBetweenPortsInBothOrders`: the crossings of the edges of
    /// two ports with the order of the ports as it is, and with the two ports switched. The
    /// counter must be set up with `init_for_counting_between` or
    /// `init_port_positions_for_in_layer_crossings_with_tree`.
    pub fn count_crossings_between_ports_in_both_orders(
        &mut self,
        graph: &LGraph,
        upper_port: PortRef,
        lower_port: PortRef,
    ) -> (f64, f64) {
        let mut ports = self.connected_ports_sorted_by_position(graph, upper_port, lower_port);
        // Java reads `indexTree` here and fails with a null pointer if the counter was not set up.
        // A missing tree is a bug in the caller, never caller input. It stays loud in release
        // builds: an empty tree would count wrong without a sign.
        let mut index_tree = self.index_tree.take().expect(
            "count_crossings_between_ports_in_both_orders needs init_for_counting_between or \
             init_port_positions_for_in_layer_crossings_with_tree first",
        );
        let upper_lower_crossings = self.count_crossings_on_ports(graph, &ports, &mut index_tree);
        // The count may add the end of an edge whose port is not in the list, so the tree must be
        // cleared explicitly.
        index_tree.clear();
        self.switch_ports(graph, upper_port, lower_port);
        // A stable sort by the new positions.
        ports.sort_by_key(|port| self.position_of(graph, *port));
        let lower_upper_crossings = self.count_crossings_on_ports(graph, &ports, &mut index_tree);
        index_tree.clear();
        self.switch_ports(graph, lower_port, upper_port);
        self.index_tree = Some(index_tree);
        (upper_lower_crossings, lower_upper_crossings)
    }

    /// `CrossingsCounter.connectedPortsSortedByPosition`. ELK collects the ports in a `TreeSet`
    /// ordered by position, so of two ports at one position only the first one stays.
    fn connected_ports_sorted_by_position(
        &self,
        graph: &LGraph,
        upper_port: PortRef,
        lower_port: PortRef,
    ) -> Vec<PortRef> {
        let mut ports: Vec<PortRef> = Vec::new();
        let add = |ports: &mut Vec<PortRef>, port: PortRef| {
            let position = self.position_of(graph, port);
            match ports.binary_search_by_key(&position, |other| self.position_of(graph, *other)) {
                Ok(_) => {}
                Err(index) => ports.insert(index, port),
            }
        };
        for port in [upper_port, lower_port] {
            add(&mut ports, port);
            for edge in connected_edges(graph, port) {
                let data = &graph.edges[edge];
                // A self-loop of a port.
                if data.source() == data.target() {
                    continue;
                }
                if let Some(other_end) = other_end_of(graph, edge, port) {
                    add(&mut ports, other_end);
                }
            }
        }
        ports
    }

    pub(crate) fn init_port_positions_counter_clockwise(
        &mut self,
        graph: &LGraph,
        left_layer_nodes: &[NodeId],
        right_layer_nodes: &[NodeId],
    ) -> Vec<PortRef> {
        let mut ports = Vec::new();
        self.init_positions(graph, left_layer_nodes, &mut ports, PortSide::East, true);
        self.init_positions(graph, right_layer_nodes, &mut ports, PortSide::West, false);
        ports
    }

    fn init_positions(
        &mut self,
        graph: &LGraph,
        nodes: &[NodeId],
        ports: &mut Vec<PortRef>,
        side: PortSide,
        top_down: bool,
    ) {
        let node_iter: Box<dyn Iterator<Item = NodeId> + '_> = if top_down {
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

    pub(crate) fn count_crossings_on_ports(
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
        nodes: &[NodeId],
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
    pub(crate) fn position_of(&self, graph: &LGraph, port: PortRef) -> usize {
        port_id(graph, port)
            .and_then(|id| self.port_positions.get(id).copied())
            .unwrap_or(0)
    }

    /// `portPositions[port.id] = position`.
    pub(crate) fn set_position(&mut self, graph: &LGraph, port: PortRef, position: usize) {
        let Some(id) = port_id(graph, port) else {
            return;
        };
        if self.port_positions.len() <= id {
            self.port_positions.resize(id + 1, 0);
        }
        self.port_positions[id] = position;
    }
}

/// `CrossingsCounter.getNorthSouthPortsWithIncidentEdges`: the ports of a side that have a dummy.
/// The sweep counts them too, as
/// `NorthSouthEdgeNeighbouringNodeCrossingsCounter.numberOfNorthSouthEdges`.
pub(crate) fn north_south_ports_with_incident_edges(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
) -> impl Iterator<Item = PortRef> + '_ {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .filter(move |(_, port)| port.side == side && port.port_dummy.is_some())
        .map(move |(port, _)| PortRef { node, port })
}

/// The first port of a node on a side, in the order of the node's port list.
pub(crate) fn first_port_on_side(graph: &LGraph, node: NodeId, side: PortSide) -> Option<PortRef> {
    graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
        .find(|(_, port)| port.side == side)
        .map(|(port, _)| PortRef { node, port })
}

/// The number of edges at a port, in and out. A port that is not in the graph has none.
pub(crate) fn port_degree(graph: &LGraph, port: PortRef) -> usize {
    let Some(port) = graph
        .layerless_nodes
        .get(port.node)
        .and_then(|node| node.ports.get(port.port))
    else {
        return 0;
    };
    port.incoming_edges().len() + port.outgoing_edges().len()
}

/// `CrossingsCounter.isLayoutUnitChanged`.
pub(crate) fn is_layout_unit_changed(
    last_unit: Option<NodeId>,
    node: NodeId,
    unit: Option<NodeId>,
) -> bool {
    let Some(last_unit) = last_unit else {
        return false;
    };
    if last_unit == node {
        return false;
    }
    unit.is_some_and(|unit| unit != last_unit)
}

/// `port.id`. Every port of a graph in the sweep has one, see `CrossingsCounter`.
pub(crate) fn port_id(graph: &LGraph, port: PortRef) -> Option<usize> {
    let id = graph.layerless_nodes[port.node].ports[port.port].crossing_minimization_id;
    debug_assert!(id.is_some(), "a port in the sweep has no crossing id");
    id
}

pub(crate) fn rank_or_nan(rank: Option<usize>) -> f64 {
    rank.map_or(f64::NAN, |rank| rank as f64)
}

pub fn ports_in_north_south_east_west_order(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
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
    if matches!(side, PortSide::South | PortSide::West) {
        ports.reverse();
    }
    ports
}

pub(crate) fn ports_for_side(
    graph: &LGraph,
    node: NodeId,
    side: PortSide,
    top_down: bool,
) -> Vec<PortRef> {
    let ports = graph.layerless_nodes[node]
        .ports
        .iter_with_ids()
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

pub(crate) fn is_in_layer_edge(graph: &LGraph, edge: EdgeId) -> bool {
    let Some(edge) = graph.edges.get(edge) else {
        return false;
    };
    graph.layerless_nodes[edge.source().node].layer_index()
        == graph.layerless_nodes[edge.target().node].layer_index()
}

pub(crate) fn connected_edges(graph: &LGraph, port: PortRef) -> impl Iterator<Item = EdgeId> + '_ {
    graph.layerless_nodes[port.node].ports[port.port]
        .incoming_edges()
        .iter()
        .chain(
            graph.layerless_nodes[port.node].ports[port.port]
                .outgoing_edges()
                .iter(),
        )
        .copied()
}

pub(crate) fn other_end_of(graph: &LGraph, edge: EdgeId, port: PortRef) -> Option<PortRef> {
    let edge = graph.edges.get(edge)?;
    if edge.source() == port {
        Some(edge.target())
    } else if edge.target() == port {
        Some(edge.source())
    } else {
        None
    }
}
