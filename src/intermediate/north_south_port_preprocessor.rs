//! Port of ELK's `NorthSouthPortPreprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/NorthSouthPortPreprocessor.java
//!
//! The dummy nodes stand for ports across the layout flow during layering.
//!
//! ELK's pre-processor also builds dummy nodes for self-loops that run between ports on a north
//! or south side. `SelfLoopPreProcessor` takes every self-loop off its ports before this step
//! runs, so no edge here ever starts and ends at the same node. Those paths are not ported, and
//! neither is the "old approach" that `USE_NEW_APPROACH` switches off.
//!
//! ELK also sets `CROSSING_HINT` on each dummy. Nothing in ELK reads it at this commit, so it has
//! no field here.

use crate::graph::{
    GraphNodeRef, LGraph, LNode, LNodeKind, LPort, NodeId, PortRef, PortSide, PortType,
};
use crate::options::PortConstraints;

/// Ports `NorthSouthPortPreprocessor.process`.
///
/// For each node with fixed port sides, a dummy node per connected north port goes above the node
/// in its layer, and one per connected south port goes below. The edges of the port move to the
/// dummy, so the layering and the crossing minimization see edges along the flow.
pub fn preprocess_north_south_ports(graph: &mut LGraph) {
    for layer_index in 0..graph.layers().len() {
        // The index of the current node while the north ports are processed, and the index of the
        // dummy placed last while the south ports are processed.
        let mut pointer: isize = -1;

        // A copy, because the loop inserts dummies into the layer.
        let node_array = graph.layers()[layer_index].nodes().to_vec();
        for node in node_array {
            pointer += 1;

            // Only regular nodes with fixed port sides.
            if !(graph.layerless_nodes[node].kind() == LNodeKind::Normal
                && graph.layerless_nodes[node].port_constraints.is_side_fixed())
            {
                continue;
            }

            // Sort the port list if we have control over the port order.
            if !graph.layerless_nodes[node]
                .port_constraints
                .is_order_fixed()
            {
                sort_port_list(graph, node);
            }

            // Nodes form their own layout unit.
            graph.tables.in_layer.row_mut(node).layout_unit = Some(node);

            let mut barycenter_associates = Vec::new();

            // The northern ports, left to right as the diagram shows them.
            let north_ports = graph.ports_on_side(node, PortSide::North);
            let north_dummies = create_dummy_nodes(graph, &north_ports, &mut barycenter_associates);

            // Each northern dummy has the regular node as its successor. A dummy whose port may
            // switch sides gets no constraint: the crossing minimization places it.
            let insert_point = pointer as usize;
            for dummy in north_dummies {
                // Every dummy goes to the same index, so the dummy made last ends up first.
                insert_into_layer(graph, dummy, layer_index, insert_point);
                pointer += 1;

                // The dummies form a layout unit named by the node they were made for.
                graph.tables.in_layer.row_mut(dummy).layout_unit = Some(node);

                if !origin_allows_side_switch(graph, dummy) {
                    graph
                        .tables
                        .in_layer
                        .row_mut(dummy)
                        .successor_constraints
                        .push(node);
                }
            }

            // The ports on the southern side are listed from right to left, so the list is built
            // in reversed order.
            let mut south_ports = graph.ports_on_side(node, PortSide::South);
            south_ports.reverse();
            let south_dummies = create_dummy_nodes(graph, &south_ports, &mut barycenter_associates);

            for dummy in south_dummies {
                pointer += 1;
                insert_into_layer(graph, dummy, layer_index, pointer as usize);

                graph.tables.in_layer.row_mut(dummy).layout_unit = Some(node);

                // The regular node has every southern dummy as its successor.
                if !origin_allows_side_switch(graph, dummy) {
                    graph
                        .tables
                        .in_layer
                        .row_mut(node)
                        .successor_constraints
                        .push(dummy);
                }
            }

            if !barycenter_associates.is_empty() {
                graph.tables.in_layer.row_mut(node).barycenter_associates = barycenter_associates;
            }
        }
    }
}

/// Whether the port a dummy was made for may switch sides.
fn origin_allows_side_switch(graph: &LGraph, dummy: NodeId) -> bool {
    graph.layerless_nodes[dummy]
        .ports
        .first()
        .and_then(|port| port.origin_port)
        .is_some_and(|origin| {
            graph.layerless_nodes[origin.node].ports[origin.port]
                .allow_non_flow_ports_to_switch_sides
        })
}

fn insert_into_layer(graph: &mut LGraph, node: NodeId, layer_index: usize, index: usize) {
    graph.insert_node_into_layer(node, layer_index, index);
}

/// Ports `NorthSouthPortPreprocessor.sortPortList`: ports with only incoming edges end up left,
/// ports with only outgoing edges end up right, and ports with both end up in between. Ports on
/// the eastern and western sides keep their order.
fn sort_port_list(graph: &mut LGraph, node: NodeId) {
    let ports = &graph.layerless_nodes[node].ports;
    let port_count = ports.len() as i64;

    // The first ids of each kind are far enough apart to stay unique.
    let mut in_ports = 0i64;
    let mut in_out_ports = port_count;
    let mut out_ports = 2 * port_count;

    let mut ids = Vec::with_capacity(ports.len());
    for port in ports {
        let id = match port.side {
            PortSide::East | PortSide::West => -1,
            PortSide::North | PortSide::South => {
                let incoming = !port.incoming_edges().is_empty();
                let outgoing = !port.outgoing_edges().is_empty();
                let next = if incoming && outgoing {
                    &mut in_out_ports
                } else if outgoing {
                    &mut out_ports
                } else {
                    // Ports without an edge are placed between the input ports.
                    &mut in_ports
                };
                let id = *next;
                *next += 1;
                id
            }
            PortSide::Undefined => 0,
        };
        ids.push(id);
    }

    let port_ids = ports.ids().collect::<Vec<_>>();
    let mut order = (0..port_ids.len()).collect::<Vec<_>>();
    // The sort is stable, as the merge sort of elkjs is.
    order.sort_by(|&first, &second| {
        let (side1, side2) = (ports[port_ids[first]].side, ports[port_ids[second]].side);
        if side1 != side2 {
            return side1.ordinal().cmp(&side2.ordinal());
        }
        if ids[first] == ids[second] {
            // Eastern and western ports have the same id and keep their order.
            std::cmp::Ordering::Equal
        } else if side1 == PortSide::North {
            ids[first].cmp(&ids[second])
        } else {
            ids[second].cmp(&ids[first])
        }
    });
    // A sort of the port indices is a permutation, so a refusal is a bug, never caller input. It
    // stays loud in release builds.
    let order = order.into_iter().map(|position| port_ids[position]);
    assert!(
        graph.reorder_node_ports(node, order),
        "the sorted port order of node {} is not a permutation of its ports",
        node.index()
    );
}

/// Ports `NorthSouthPortPreprocessor.createDummyNodes`, for the new approach.
///
/// `ports` runs from left to right. The dummies come out in this order: the ports with only
/// incoming edges, then those with only outgoing edges, then those with both. Each dummy is also
/// a barycenter associate of the node.
fn create_dummy_nodes(
    graph: &mut LGraph,
    ports: &[PortRef],
    barycenter_associates: &mut Vec<NodeId>,
) -> Vec<NodeId> {
    let mut in_ports = Vec::new();
    let mut out_ports = Vec::new();
    let mut in_out_ports = Vec::new();
    for &port in ports {
        let data = &graph.layerless_nodes[port.node].ports[port.port];
        let incoming = !data.incoming_edges().is_empty();
        let outgoing = !data.outgoing_edges().is_empty();
        if incoming && outgoing {
            in_out_ports.push(port);
        } else if incoming {
            in_ports.push(port);
        } else if outgoing {
            out_ports.push(port);
        }
    }

    let mut dummies = Vec::new();
    for port in in_ports {
        let dummy = create_dummy_node(graph, Some(port), None);
        barycenter_associates.push(dummy);
        dummies.push(dummy);
    }
    for port in out_ports {
        let dummy = create_dummy_node(graph, None, Some(port));
        barycenter_associates.push(dummy);
        dummies.push(dummy);
    }
    for port in in_out_ports {
        let dummy = create_dummy_node(graph, Some(port), Some(port));
        barycenter_associates.push(dummy);
        dummies.push(dummy);
    }
    dummies
}

/// Ports `NorthSouthPortPreprocessor.createDummyNode`.
///
/// The edges that enter `in_port` now enter the dummy's west port. The edges that leave
/// `out_port` now leave the dummy's east port. Both arguments may name the same port.
fn create_dummy_node(
    graph: &mut LGraph,
    in_port: Option<PortRef>,
    out_port: Option<PortRef>,
) -> NodeId {
    let dummy_index = graph.push_node_with(|id| {
        let mut dummy = LNode::new(format!("northSouthPort:{}", id.index()), 0.0, 0.0, None);
        dummy.set_kind(LNodeKind::NorthSouthPort);
        dummy.port_constraints = PortConstraints::FixedPos;
        dummy
    });
    let dummy_ref = GraphNodeRef {
        graph_id: graph.id,
        node: dummy_index,
    };

    if let Some(in_port) = in_port {
        let dummy_port = push_dummy_port(graph, dummy_index, PortType::Input, PortSide::West);
        graph.layerless_nodes[dummy_index].ports[dummy_port.port].origin_port = Some(in_port);

        let edges = graph.layerless_nodes[in_port.node].ports[in_port.port]
            .incoming_edges()
            .to_vec();
        for edge in edges {
            graph.set_edge_target(edge, dummy_port);
        }
        graph.layerless_nodes[in_port.node].ports[in_port.port].port_dummy = Some(dummy_ref);
    }

    if let Some(out_port) = out_port {
        let dummy_port = push_dummy_port(graph, dummy_index, PortType::Output, PortSide::East);
        graph.layerless_nodes[dummy_index].ports[dummy_port.port].origin_port = Some(out_port);

        let edges = graph.layerless_nodes[out_port.node].ports[out_port.port]
            .outgoing_edges()
            .to_vec();
        for edge in edges {
            graph.set_edge_source(edge, dummy_port);
        }
        graph.layerless_nodes[out_port.node].ports[out_port.port].port_dummy = Some(dummy_ref);
    }

    dummy_index
}

pub(crate) fn push_dummy_port(
    graph: &mut LGraph,
    dummy: NodeId,
    port_type: PortType,
    side: PortSide,
) -> PortRef {
    let node = &mut graph.layerless_nodes[dummy];
    // `slot_count`, like `LGraph::add_port`: a port off the list still holds its name.
    let index = node.ports.slot_count();
    let mut port = LPort::new(format!("{}:{index}", node.id), dummy, port_type);
    port.set_side(side);
    PortRef {
        node: dummy,
        port: node.ports.push(port),
    }
}
