//! Phase 3 crossing counters.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/BinaryIndexedTree.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/CrossingsCounter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/AllCrossingsCounter.java

use std::collections::HashMap;

use crate::graph::{EdgeId, EdgeSlice, LGraph, LNodeKind, NodeId, NodeSlice, PortRef, PortSide};

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

    /// `BinaryIndexedTree.clear`: removes every entry.
    pub fn clear(&mut self) {
        self.binary_sums.fill(0);
        self.nums_per_index.fill(0);
        self.size = 0;
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
    /// `AllCrossingsCounter.hasHyperEdgesEastOfIndex`. ELK sets it once, in `initAtPortLevel`,
    /// when it builds the counter. The first count sets it here, and no later count reads the
    /// ports again: the edges at a port do not change during the sweep.
    has_hyperedges_east_of_index: Option<Vec<bool>>,
    /// `AllCrossingsCounter.hasNorthSouthPorts`, set in `initAtNodeLevel` when ELK builds the
    /// counter. The first count sets it here, like `has_hyperedges_east_of_index`.
    has_north_south_ports: Option<Vec<bool>>,
    /// The tree that `initForCountingBetween` and `initPortPositionsForInLayerCrossings` make for
    /// the counts of single port pairs.
    index_tree: Option<BinaryIndexedTree>,
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
            match node.kind {
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
    fn init_positions_for_north_south_counting(
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
            if is_layout_unit_changed(last_layout_unit, current, node.in_layer_layout_unit) {
                // Work the stack, which is full of southern dummies.
                index = self.empty_stack(graph, &mut stack, &mut ports, STACK_SIDE, index);
            }
            if node.in_layer_layout_unit.is_some() {
                last_layout_unit = node.in_layer_layout_unit;
            }

            match node.kind {
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
                if data.source == data.target {
                    continue;
                }
                if let Some(other_end) = other_end_of(graph, edge, port) {
                    add(&mut ports, other_end);
                }
            }
        }
        ports
    }

    fn init_port_positions_counter_clockwise(
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

    /// `HyperedgeCrossingsCounter.countCrossings`. The number is exact for plain edges and an
    /// estimate for hyperedges, as in ELK. Like ELK's `int`, it can be negative: a hyperedge with
    /// no port in one of the two layers has its upper corner below its lower corner.
    ///
    /// ELK breaks a tie between two hyperedges or corners by `hashCode()`. That tie cannot happen
    /// here: each hyperedge owns its own ports, so the positions it is sorted by never repeat.
    fn count_hyperedge_crossings(
        &mut self,
        graph: &LGraph,
        left_layer: &[NodeId],
        right_layer: &[NodeId],
    ) -> isize {
        let layer_of = |node: NodeId| graph.layerless_nodes[node].layer_index;
        let (Some(&left_first), Some(&right_first)) = (left_layer.first(), right_layer.first())
        else {
            return 0;
        };
        let left_layer_ref = layer_of(left_first);
        let right_layer_ref = layer_of(right_first);

        // Assign index values to the ports of the left layer, in the order north - east - south - west.
        let mut source_count = 0usize;
        for node in left_layer {
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter_with_ids() {
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
            for (port_index, port) in ports.iter_with_ids().rev() {
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
            for (port_index, port) in graph.layerless_nodes[*node].ports.iter_with_ids() {
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
        let mut crossings = 0isize;
        for position in &south_sequence {
            let mut index = position + first_index;
            tree[index] += 1;
            while index > 0 {
                if !index.is_multiple_of(2) {
                    crossings += tree[index + 1] as isize;
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
        .get_node(port.node)
        .and_then(|node| node.ports.get(port.port))
    else {
        return 0;
    };
    port.incoming_edges.len() + port.outgoing_edges.len()
}

/// `CrossingsCounter.isLayoutUnitChanged`.
fn is_layout_unit_changed(last_unit: Option<NodeId>, node: NodeId, unit: Option<NodeId>) -> bool {
    let Some(last_unit) = last_unit else {
        return false;
    };
    if last_unit == node {
        return false;
    }
    unit.is_some_and(|unit| unit != last_unit)
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

/// `AllCrossingsCounter.initAtNodeLevel`: `hasNorthSouthPorts[l]` is set when layer `l` has a
/// north or south port dummy.
fn has_north_south_ports(graph: &LGraph, order: &[Vec<NodeId>]) -> Vec<bool> {
    order
        .iter()
        .map(|layer| {
            layer
                .iter()
                .any(|&node| graph.layerless_nodes[node].kind == LNodeKind::NorthSouthPort)
        })
        .collect()
}

/// `AllCrossingsCounter.initAtPortLevel`: `hasHyperEdgesEastOfIndex[l]` is set when an east port of
/// layer `l`, or a west port of layer `l + 1`, has more than one edge. Only real ports can have
/// that; a port ELK makes up for a plain edge has exactly one.
fn has_hyperedges_east_of_index(graph: &LGraph, order: &[Vec<NodeId>]) -> Vec<bool> {
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
///
/// The sum can be negative, as ELK's `int` can: a span whose upper corner lies below its lower
/// corner closes before it opens.
fn count_overlapping_areas(spans: &[(usize, usize)]) -> isize {
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
    crossings
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

fn ports_for_side(graph: &LGraph, node: NodeId, side: PortSide, top_down: bool) -> Vec<PortRef> {
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

fn is_in_layer_edge(graph: &LGraph, edge: EdgeId) -> bool {
    let Some(edge) = graph.edges.get_edge(edge) else {
        return false;
    };
    graph.layerless_nodes[edge.source.node].layer_index
        == graph.layerless_nodes[edge.target.node].layer_index
}

fn connected_edges(graph: &LGraph, port: PortRef) -> impl Iterator<Item = EdgeId> + '_ {
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

fn other_end_of(graph: &LGraph, edge: EdgeId, port: PortRef) -> Option<PortRef> {
    let edge = graph.edges.get_edge(edge)?;
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
    use crate::graph::PortId;
    use crate::graph::{LGraph, LNode, LPoint, LayeredEdge, PortType};
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::options::{ElkDirection, LayeredOptions, PortConstraints};
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

    fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> crate::graph::GraphArena {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap();
        layer_network_simplex(&mut graph).unwrap();
        process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
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
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
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
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
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
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
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
        let top = graph.node_named("Top");
        let bottom = graph.node_named("Bottom");
        let left = graph.node_named("Left");
        let right = graph.node_named("Right");
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
    fn graph_with_a_west_port_that_leaves() -> (LGraph, Vec<Vec<NodeId>>) {
        graph_with_a_west_port_that_leaves_and_idle_ports_on_b(0)
    }

    /// The same graph, with `idle` more west ports on the last node. They have no edge. They make
    /// the tree of the last pair as big as the number of its ports.
    fn graph_with_a_west_port_that_leaves_and_idle_ports_on_b(
        idle: usize,
    ) -> (LGraph, Vec<Vec<NodeId>>) {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for id in ["a1", "a2", "n", "b"] {
            graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
        }
        for (node, layer) in [(0, 0), (1, 0), (2, 1), (3, 2)] {
            graph.set_node_layer(NodeId::new(node), layer);
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
        let (a1_out, a2_out) = (
            out_of(&mut graph, NodeId::new(0)),
            out_of(&mut graph, NodeId::new(1)),
        );
        let (n_in1, n_in2) = (
            into(&mut graph, NodeId::new(2), PortSide::West),
            into(&mut graph, NodeId::new(2), PortSide::West),
        );
        let n_leaves = graph
            .add_port(
                NodeId::new(2),
                PortType::Output,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        let b_in = into(&mut graph, NodeId::new(3), PortSide::West);
        for _ in 0..idle {
            into(&mut graph, NodeId::new(3), PortSide::West);
        }
        graph.add_edge(layered_edge("a1-n", a1_out, n_in1));
        graph.add_edge(layered_edge("a2-n", a2_out, n_in2));
        graph.add_edge(layered_edge("n-b", n_leaves, b_in));
        let order = vec![
            vec![NodeId::new(0), NodeId::new(1)],
            vec![NodeId::new(2)],
            vec![NodeId::new(3)],
        ];
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
        assert!(graph.reorder_node_ports(NodeId::new(2), crate::graph::port_ids([2, 0, 1])));
        // The last pair holds three ports, so its tree has no place 4: the count is NaN. The port
        // that now sits at the old place of the front port would read 2 and give 0.
        assert!(
            counter
                .count_crossings_between_layers(&graph, &order[1], &order[2])
                .is_nan()
        );
    }

    // Mutation: work out the hyperedge flags again at each count, instead of keeping the ones of
    // the first count. The second count then sees the second edge at the port and sets the flag.
    #[test]
    fn the_hyperedge_flags_are_set_by_the_first_count_only() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for id in ["source", "target"] {
            graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
        }
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 1);
        let out = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let in_top = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        let in_bottom = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        graph.add_edge(layered_edge("first", out, in_top));
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        let order = vec![vec![NodeId::new(0)], vec![NodeId::new(1)]];

        let mut counter = CrossingsCounter::default();
        assert_eq!(counter.has_hyperedges_east_of_index, None);
        counter.count_all_crossings_in_order(&graph, &order);
        assert_eq!(counter.has_hyperedges_east_of_index, Some(vec![false; 2]));

        // A second edge at the east port makes a hyperedge, which a new counter sees at once.
        graph.add_edge(layered_edge("second", out, in_bottom));
        let mut fresh = CrossingsCounter::default();
        fresh.count_all_crossings_in_order(&graph, &order);
        assert_eq!(fresh.has_hyperedges_east_of_index, Some(vec![true, false]));
        // The old counter keeps the flags it set, as ELK's counter keeps those of its construction.
        counter.count_all_crossings_in_order(&graph, &order);
        assert_eq!(counter.has_hyperedges_east_of_index, Some(vec![false; 2]));
    }

    // Mutation: look for north and south port dummies again at each count, instead of keeping the
    // flags of the first count. The old counter then sees the dummy that came later.
    #[test]
    fn the_north_south_flags_are_set_by_the_first_count_only() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("a", 10.0, 10.0, None));
        graph.set_node_layer(NodeId::new(0), 0);
        crate::p3order::initialize_crossing_minimization_port_ids(&mut graph);
        let order = vec![vec![NodeId::new(0)]];

        let mut counter = CrossingsCounter::default();
        assert_eq!(counter.has_north_south_ports, None);
        counter.count_all_crossings_in_order(&graph, &order);
        assert_eq!(counter.has_north_south_ports, Some(vec![false]));

        // A dummy that shows up later is seen by a new counter at once.
        graph.layerless_nodes[0].kind = LNodeKind::NorthSouthPort;
        let mut fresh = CrossingsCounter::default();
        fresh.count_all_crossings_in_order(&graph, &order);
        assert_eq!(fresh.has_north_south_ports, Some(vec![true]));
        // The old counter keeps the flags it set, as ELK's counter keeps those of its construction.
        counter.count_all_crossings_in_order(&graph, &order);
        assert_eq!(counter.has_north_south_ports, Some(vec![false]));
    }

    // Mutation: clamp the sum of `count_overlapping_areas` at 0, as the port did. The span that
    // closes before it opens then adds nothing, and the sum of two sides is 0 and not -1.
    #[test]
    fn overlapping_areas_can_sum_to_less_than_zero() {
        // Upper below lower: the lower corner sorts first, and it closes a span that is not open.
        assert_eq!(count_overlapping_areas(&[(5, 3)]), -1);
        // Two sides add as signed numbers: one crossing from the sorted spans, minus one.
        assert_eq!(count_overlapping_areas(&[(0, 4), (1, 3)]), 1);
        assert_eq!(
            count_overlapping_areas(&[(0, 4), (1, 3)]) + count_overlapping_areas(&[(5, 3)]),
            0
        );
        // An ordinary span closes after it opens.
        assert_eq!(count_overlapping_areas(&[(1, 3)]), 0);
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
        graph.set_node_layer(NodeId::new(0), 0);
        graph.set_node_layer(NodeId::new(1), 1);

        let source_top = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let source_bottom = graph
            .add_port(
                NodeId::new(0),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let target_bottom = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        let target_top = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();

        graph.add_edge(layered_edge("top-to-bottom", source_top, target_bottom));
        graph.add_edge(layered_edge("bottom-to-top", source_bottom, target_top));

        let crossings = CrossingsCounter::default().count_all_crossings(&graph);

        assert_eq!(crossings, 1.0);
    }

    // --- North and south port dummies. ---

    /// A layer 0 node with two ports to attach the neighbours of the dummies' nodes to.
    fn ns_neighbour(graph: &mut LGraph) -> NodeId {
        let index = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("neighbour", 10.0, 10.0, None));
        graph.set_node_layer(index, 0);
        for _ in 0..4 {
            graph.add_port(index, PortType::Output, PortSide::East, LPoint::default());
        }
        index
    }

    /// A layer 1 node with fixed port sides and one port per side given.
    fn ns_node(graph: &mut LGraph, id: &str, sides: &[PortSide]) -> NodeId {
        let index = graph.next_node_id();
        let mut node = LNode::new(id, 40.0, 20.0, None);
        node.port_constraints = PortConstraints::FixedSide;
        graph.layerless_nodes.push(node);
        graph.set_node_layer(index, 1);
        for side in sides {
            graph.add_port(index, PortType::Input, *side, LPoint::default());
        }
        index
    }

    fn ns_graph() -> LGraph {
        LGraph::new(
            "g",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        )
    }

    /// Sets the order of layer 1 to the given nodes.
    fn set_layer_order(graph: &mut LGraph, order: &[NodeId]) {
        assert_eq!(graph.layers[1].nodes.len(), order.len());
        graph.layers[1].nodes = order.to_vec();
    }

    fn dummy_for(graph: &LGraph, node: NodeId, port: usize) -> NodeId {
        graph.layerless_nodes[node].ports[PortId::new(port)]
            .port_dummy
            .as_ref()
            .unwrap()
            .node
    }

    fn ns_crossings(graph: &LGraph) -> f64 {
        let graph = numbered(graph);
        let layer = graph.layers[1].nodes.clone();
        CrossingsCounter::default().count_north_south_port_crossings_in_layer(&graph, &layer)
    }

    /// The graph with an id on every port, as the sweep gives them before it counts.
    fn numbered(graph: &LGraph) -> LGraph {
        let mut graph = graph.clone();
        let ports = graph
            .layerless_nodes
            .iter_mut()
            .flat_map(|node| node.ports.slots_mut());
        for (id, port) in ports.enumerate() {
            port.crossing_minimization_id = Some(id);
        }
        graph
    }

    /// Two nodes with a north port each. Every port has `edges_b` edges for the second node.
    /// Returns the graph, the nodes and their dummies.
    fn two_north_nodes(edges_b: usize) -> (LGraph, [NodeId; 2], [NodeId; 2]) {
        let mut graph = ns_graph();
        let neighbour = ns_neighbour(&mut graph);
        let a = ns_node(&mut graph, "a", &[PortSide::North]);
        let b = ns_node(&mut graph, "b", &[PortSide::North]);
        graph.add_edge(layered_edge(
            "ea",
            PortRef {
                node: neighbour,
                port: PortId::new(0),
            },
            PortRef {
                node: a,
                port: PortId::new(0),
            },
        ));
        for index in 0..edges_b {
            graph.add_edge(layered_edge(
                &format!("eb{index}"),
                PortRef {
                    node: neighbour,
                    port: PortId::new(1 + index),
                },
                PortRef {
                    node: b,
                    port: PortId::new(0),
                },
            ));
        }
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let dummies = [dummy_for(&graph, a, 0), dummy_for(&graph, b, 0)];
        (graph, [a, b], dummies)
    }

    // Mutation: skip the count for the north and south ports of a layer.
    #[test]
    fn a_dummy_next_to_its_node_causes_no_north_south_crossing() {
        let (graph, _, _) = two_north_nodes(1);
        assert_eq!(ns_crossings(&graph), 0.0);
    }

    // Mutation: index the dummies after the northern ports, or count the nested pairs as crossings.
    #[test]
    fn two_northern_dummies_in_the_wrong_order_cross_once() {
        let (mut graph, [a, b], [da, db]) = two_north_nodes(1);
        set_layer_order(&mut graph, &[da, db, a, b]);
        assert_eq!(ns_crossings(&graph), 1.0);
        // The other order nests the edges.
        set_layer_order(&mut graph, &[db, da, a, b]);
        assert_eq!(ns_crossings(&graph), 0.0);
    }

    // Mutation: count the pair of ports once for every pair of edges instead of weighing it by
    // the number of edges at the dummy's port.
    #[test]
    fn a_crossing_counts_once_for_each_edge_at_the_dummy_port() {
        let (mut graph, [a, b], [da, db]) = two_north_nodes(3);
        set_layer_order(&mut graph, &[da, db, a, b]);
        // The dummy of `b` has three edges, so its implicit edge counts three times.
        assert_eq!(ns_crossings(&graph), 3.0);
    }

    /// A long edge dummy in layer 1, with its two neighbours in layers 0 and 2.
    fn add_long_edge_dummy(graph: &mut LGraph, neighbour: NodeId) -> NodeId {
        let index = graph.next_node_id();
        let mut dummy = LNode::new("long", 0.0, 0.0, None);
        dummy.kind = LNodeKind::LongEdge;
        graph.layerless_nodes.push(dummy);
        graph.set_node_layer(index, 1);
        let west = graph
            .add_port(index, PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let east = graph
            .add_port(index, PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        graph.add_edge(layered_edge(
            "into-long",
            PortRef {
                node: neighbour,
                port: PortId::new(3),
            },
            west,
        ));
        let far = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("far", 10.0, 10.0, None));
        graph.set_node_layer(far, 2);
        let far_port = graph
            .add_port(far, PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        graph.add_edge(layered_edge("out-of-long", east, far_port));
        index
    }

    // Mutation: ignore long edge dummies when numbering the ports, or number the eastern port of
    // a long edge dummy right after its western one.
    #[test]
    fn a_long_edge_crosses_the_edge_between_a_dummy_and_its_node_only_when_they_interleave() {
        let mut graph = ns_graph();
        let neighbour = ns_neighbour(&mut graph);
        let a = ns_node(&mut graph, "a", &[PortSide::North]);
        graph.add_edge(layered_edge(
            "ea",
            PortRef {
                node: neighbour,
                port: PortId::new(0),
            },
            PortRef {
                node: a,
                port: PortId::new(0),
            },
        ));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let da = dummy_for(&graph, a, 0);
        let long = add_long_edge_dummy(&mut graph, neighbour);

        // The long edge passes between the dummy and its node.
        set_layer_order(&mut graph, &[da, long, a]);
        assert_eq!(ns_crossings(&graph), 1.0);
        // The long edge passes the node altogether.
        set_layer_order(&mut graph, &[long, da, a]);
        assert_eq!(ns_crossings(&graph), 0.0);
    }

    // Mutation: index the southern ports like the northern ones, or leave the eastern ports of
    // long edge dummies out of the stack.
    #[test]
    fn a_southern_dummy_and_a_long_edge_cross_when_they_interleave() {
        let mut graph = ns_graph();
        let neighbour = ns_neighbour(&mut graph);
        let a = ns_node(&mut graph, "a", &[PortSide::South]);
        graph.add_edge(layered_edge(
            "ea",
            PortRef {
                node: neighbour,
                port: PortId::new(0),
            },
            PortRef {
                node: a,
                port: PortId::new(0),
            },
        ));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let da = dummy_for(&graph, a, 0);
        let long = add_long_edge_dummy(&mut graph, neighbour);

        set_layer_order(&mut graph, &[a, long, da]);
        assert_eq!(ns_crossings(&graph), 1.0);
        set_layer_order(&mut graph, &[long, a, da]);
        assert_eq!(ns_crossings(&graph), 0.0);
    }

    // Mutation: number the southern ports of a node from west to east.
    #[test]
    fn the_southern_ports_of_a_node_are_numbered_from_east_to_west() {
        let mut graph = ns_graph();
        let neighbour = ns_neighbour(&mut graph);
        let a = ns_node(&mut graph, "a", &[PortSide::South, PortSide::South]);
        for port in 0..2 {
            graph.add_edge(layered_edge(
                &format!("e{port}"),
                PortRef {
                    node: neighbour,
                    port: PortId::new(port),
                },
                PortRef {
                    node: a,
                    port: PortId::new(port),
                },
            ));
        }
        crate::north_south::preprocess_north_south_ports(&mut graph);
        // The first port of the list is the eastern one, which is port 1. Its dummy is the last in
        // the layer.
        let (east, west) = (dummy_for(&graph, a, 1), dummy_for(&graph, a, 0));
        assert_eq!(graph.layers[1].nodes, vec![a, west, east]);
        assert_eq!(ns_crossings(&graph), 0.0);

        set_layer_order(&mut graph, &[a, east, west]);
        assert_eq!(ns_crossings(&graph), 1.0);
    }

    // Mutation: never empty the stack when the layout unit changes, so that the eastern port of a
    // dummy gets its index after the ports of the next unit.
    #[test]
    fn the_stack_of_eastern_dummy_ports_is_emptied_when_the_layout_unit_changes() {
        let mut graph = ns_graph();
        let neighbour = ns_neighbour(&mut graph);
        let a = ns_node(&mut graph, "a", &[PortSide::South]);
        let b = ns_node(&mut graph, "b", &[PortSide::North]);
        // The south port of `a` has an outgoing edge, so its dummy has an eastern port only.
        graph.add_edge(layered_edge(
            "from-a",
            PortRef {
                node: a,
                port: PortId::new(0),
            },
            PortRef {
                node: neighbour,
                port: PortId::new(1),
            },
        ));
        graph.add_edge(layered_edge(
            "to-b",
            PortRef {
                node: neighbour,
                port: PortId::new(0),
            },
            PortRef {
                node: b,
                port: PortId::new(0),
            },
        ));
        crate::north_south::preprocess_north_south_ports(&mut graph);
        let da = dummy_for(&graph, a, 0);
        let db = dummy_for(&graph, b, 0);
        set_layer_order(&mut graph, &[a, da, db, b]);

        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        let layer = graph.layers[1].nodes.clone();
        let ports = counter.init_positions_for_north_south_counting(&graph, &layer);

        // `a`'s southern port, the eastern port of its dummy, the western port of the dummy of
        // `b`, then the northern port of `b`.
        let expected = vec![
            PortRef {
                node: a,
                port: PortId::new(0),
            },
            PortRef {
                node: da,
                port: PortId::new(0),
            },
            PortRef {
                node: db,
                port: PortId::new(0),
            },
            PortRef {
                node: b,
                port: PortId::new(0),
            },
        ];
        assert_eq!(ports, expected);
        assert_eq!(counter.position_of(&graph, expected[1]), 1);
    }

    // Mutation: leave the north and south count out of `count_all_crossings_in_order`.
    #[test]
    fn all_crossings_include_those_of_north_and_south_ports() {
        let (mut graph, [a, b], [da, db]) = two_north_nodes(1);
        set_layer_order(&mut graph, &[da, db, a, b]);
        let order = graph
            .layers
            .iter()
            .map(|layer| layer.nodes.clone())
            .collect::<Vec<_>>();
        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        let between = counter.count_crossings_between_layers(&graph, &order[0], &order[1]);
        let total = counter.count_all_crossings_in_order(&graph, &order);
        // The edges into the two dummies leave the neighbour in the order of the ports, and the
        // dummies are in the other order.
        assert_eq!(total, between + 1.0);
        assert_eq!(
            counter.count_north_south_port_crossings_in_layer(&graph, &order[1]),
            1.0
        );
    }

    // Mutation: skip the count of the eastern side of the last layer when there is only one layer.
    #[test]
    fn a_single_layer_counts_the_crossings_of_its_eastern_in_layer_edges() {
        let mut graph = ns_graph();
        let mut nodes = Vec::new();
        for index in 0..4 {
            let node = graph.next_node_id();
            graph
                .layerless_nodes
                .push(LNode::new(format!("n{index}"), 10.0, 10.0, None));
            graph.set_node_layer(node, 0);
            graph.add_port(node, PortType::Output, PortSide::East, LPoint::default());
            nodes.push(node);
        }
        // n0 to n2 and n1 to n3 cross on the eastern side.
        graph.add_edge(layered_edge(
            "a",
            PortRef {
                node: nodes[0],
                port: PortId::new(0),
            },
            PortRef {
                node: nodes[2],
                port: PortId::new(0),
            },
        ));
        graph.add_edge(layered_edge(
            "b",
            PortRef {
                node: nodes[1],
                port: PortId::new(0),
            },
            PortRef {
                node: nodes[3],
                port: PortId::new(0),
            },
        ));

        let graph = numbered(&graph);
        assert_eq!(CrossingsCounter::default().count_all_crossings(&graph), 1.0);
    }

    // --- Counts for a pair of ports. ---

    // Mutation: leave the sums or the size alone in `clear`.
    #[test]
    fn clearing_a_binary_indexed_tree_removes_every_entry() {
        let mut tree = BinaryIndexedTree::new(4);
        tree.add(1);
        tree.add(1);
        tree.add(3);
        tree.clear();
        assert_eq!(tree.size(), 0);
        assert_eq!(tree.rank(4), Some(0));
        tree.add(2);
        assert_eq!(tree.rank(3), Some(1));
        assert_eq!(tree.rank(2), Some(0));
    }

    /// Two nodes, `left` with two east ports and `right` with two west ports. The ports of
    /// `right` are listed from bottom to top. `left.0` is the upper port of `left`.
    /// Returns the graph and the four ports as (left upper, left lower, right upper, right lower).
    fn two_by_two(edges: [(usize, usize); 2]) -> (LGraph, [PortRef; 4]) {
        let mut graph = ns_graph();
        let left = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("left", 10.0, 10.0, None));
        graph.set_node_layer(left, 0);
        let right = graph.next_node_id();
        graph
            .layerless_nodes
            .push(LNode::new("right", 10.0, 10.0, None));
        graph.set_node_layer(right, 1);
        for _ in 0..2 {
            graph.add_port(left, PortType::Output, PortSide::East, LPoint::default());
            graph.add_port(right, PortType::Input, PortSide::West, LPoint::default());
        }
        let left_ports = [
            PortRef {
                node: left,
                port: PortId::new(0),
            },
            PortRef {
                node: left,
                port: PortId::new(1),
            },
        ];
        // The list of the west side runs from bottom to top: port 1 is the upper one.
        let right_ports = [
            PortRef {
                node: right,
                port: PortId::new(1),
            },
            PortRef {
                node: right,
                port: PortId::new(0),
            },
        ];
        for (index, (from, to)) in edges.into_iter().enumerate() {
            graph.add_edge(layered_edge(
                &format!("e{index}"),
                left_ports[from],
                right_ports[to],
            ));
        }
        (
            graph,
            [left_ports[0], left_ports[1], right_ports[0], right_ports[1]],
        )
    }

    // Mutation: report the crossings of the switched order first, or leave the ports switched
    // after the count.
    #[test]
    fn a_pair_of_ports_is_counted_in_both_orders_and_left_as_it_was() {
        // The upper left port goes to the lower right port, and the other way round: crossing.
        let (graph, [left_upper, left_lower, right_upper, right_lower]) =
            two_by_two([(0, 1), (1, 0)]);
        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        counter.init_for_counting_between(&graph, &[NodeId::new(0)], &[NodeId::new(1)]);
        let before = (
            counter.position_of(&graph, left_upper),
            counter.position_of(&graph, left_lower),
        );

        let counts =
            counter.count_crossings_between_ports_in_both_orders(&graph, left_upper, left_lower);

        // As they are the edges cross once; switched they do not.
        assert_eq!(counts, (1.0, 0.0));
        assert_eq!(
            (
                counter.position_of(&graph, left_upper),
                counter.position_of(&graph, left_lower)
            ),
            before
        );
        // The tree is clear for the next count, which finds the same.
        assert_eq!(
            counter.count_crossings_between_ports_in_both_orders(&graph, left_upper, left_lower),
            (1.0, 0.0)
        );
        // Without a crossing it is the other way round.
        let (graph, [left_upper, left_lower, ..]) = two_by_two([(0, 0), (1, 1)]);
        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        counter.init_for_counting_between(&graph, &[NodeId::new(0)], &[NodeId::new(1)]);
        assert_eq!(
            counter.count_crossings_between_ports_in_both_orders(&graph, left_upper, left_lower),
            (0.0, 1.0)
        );
        let _ = (right_upper, right_lower);
    }

    // Mutation: make an empty tree when the counter has none (`unwrap_or_else(||
    // BinaryIndexedTree::new(0))`), so a counter that was never set up counts without a word.
    #[test]
    #[should_panic(expected = "needs init_for_counting_between")]
    fn counting_a_pair_of_ports_without_setting_the_counter_up_fails_loudly() {
        let (graph, [left_upper, left_lower, ..]) = two_by_two([(0, 1), (1, 0)]);
        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        counter.count_crossings_between_ports_in_both_orders(&graph, left_upper, left_lower);
    }

    // Mutation: swap one position and leave the other.
    #[test]
    fn switching_two_ports_swaps_their_positions() {
        let (graph, [left_upper, left_lower, ..]) = two_by_two([(0, 0), (1, 1)]);
        let graph = numbered(&graph);
        let mut counter = CrossingsCounter::default();
        counter.init_for_counting_between(&graph, &[NodeId::new(0)], &[NodeId::new(1)]);
        let (upper, lower) = (
            counter.position_of(&graph, left_upper),
            counter.position_of(&graph, left_lower),
        );
        assert_ne!(upper, lower);

        counter.switch_ports(&graph, left_upper, left_lower);

        assert_eq!(counter.position_of(&graph, left_upper), lower);
        assert_eq!(counter.position_of(&graph, left_lower), upper);
    }
}
