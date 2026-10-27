//! Ports across the layout flow: the dummy nodes that stand for them during layering, and the
//! step that gives the edges back to the ports afterwards.
//!
//! Source references (62d5909f96fad541bc101ad52dabaece6b7eab7e):
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/NorthSouthPortPreprocessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/NorthSouthPortPostprocessor.java
//!
//! ELK's pre-processor also builds dummy nodes for self-loops that run between ports on a north
//! or south side. `SelfLoopPreProcessor` takes every self-loop off its ports before this step
//! runs, so no edge here ever starts and ends at the same node. Those paths are not ported, and
//! neither is the "old approach" that `USE_NEW_APPROACH` switches off. The spline branch of the
//! post-processor waits for the spline router, which is not ported.
//!
//! ELK also sets `CROSSING_HINT` on each dummy. Nothing in ELK reads it at this commit, so it has
//! no field here.

use crate::graph::{
    GraphNodeRef, LGraph, LNode, LNodeKind, LPoint, LPort, PortRef, PortSide, PortType,
};
use crate::options::PortConstraints;

/// Ports `NorthSouthPortPreprocessor.process`.
///
/// For each node with fixed port sides, a dummy node per connected north port goes above the node
/// in its layer, and one per connected south port goes below. The edges of the port move to the
/// dummy, so the layering and the crossing minimization see edges along the flow.
pub fn preprocess_north_south_ports(graph: &mut LGraph) {
    for layer_index in 0..graph.layers.len() {
        // The index of the current node while the north ports are processed, and the index of the
        // dummy placed last while the south ports are processed.
        let mut pointer: isize = -1;

        // A copy, because the loop inserts dummies into the layer.
        let node_array = graph.layers[layer_index].nodes.clone();
        for node in node_array {
            pointer += 1;

            // Only regular nodes with fixed port sides.
            if !(graph.layerless_nodes[node].kind == LNodeKind::Normal
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
            graph.layerless_nodes[node].in_layer_layout_unit = Some(node);

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
                graph.layerless_nodes[dummy].in_layer_layout_unit = Some(node);

                if !origin_allows_side_switch(graph, dummy) {
                    graph.layerless_nodes[dummy]
                        .in_layer_successor_constraints
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

                graph.layerless_nodes[dummy].in_layer_layout_unit = Some(node);

                // The regular node has every southern dummy as its successor.
                if !origin_allows_side_switch(graph, dummy) {
                    graph.layerless_nodes[node]
                        .in_layer_successor_constraints
                        .push(dummy);
                }
            }

            if !barycenter_associates.is_empty() {
                graph.layerless_nodes[node].barycenter_associates = barycenter_associates;
            }
        }
    }
}

/// Whether the port a dummy was made for may switch sides.
fn origin_allows_side_switch(graph: &LGraph, dummy: usize) -> bool {
    graph.layerless_nodes[dummy]
        .ports
        .first()
        .and_then(|port| port.origin_port)
        .is_some_and(|origin| {
            graph.layerless_nodes[origin.node].ports[origin.port]
                .allow_non_flow_ports_to_switch_sides
        })
}

fn insert_into_layer(graph: &mut LGraph, node: usize, layer_index: usize, index: usize) {
    graph.layers[layer_index].nodes.insert(index, node);
    graph.layerless_nodes[node].layer_index = Some(layer_index);
}

/// Ports `NorthSouthPortPreprocessor.sortPortList`: ports with only incoming edges end up left,
/// ports with only outgoing edges end up right, and ports with both end up in between. Ports on
/// the eastern and western sides keep their order.
fn sort_port_list(graph: &mut LGraph, node: usize) {
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
                let incoming = !port.incoming_edges.is_empty();
                let outgoing = !port.outgoing_edges.is_empty();
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
        "the sorted port order of node {node} is not a permutation of its ports"
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
    barycenter_associates: &mut Vec<usize>,
) -> Vec<usize> {
    let mut in_ports = Vec::new();
    let mut out_ports = Vec::new();
    let mut in_out_ports = Vec::new();
    for &port in ports {
        let data = &graph.layerless_nodes[port.node].ports[port.port];
        let incoming = !data.incoming_edges.is_empty();
        let outgoing = !data.outgoing_edges.is_empty();
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
) -> usize {
    let dummy_index = graph.layerless_nodes.len();
    let mut dummy = LNode::new(format!("northSouthPort:{dummy_index}"), 0.0, 0.0, None);
    dummy.kind = LNodeKind::NorthSouthPort;
    dummy.port_constraints = PortConstraints::FixedPos;
    graph.layerless_nodes.push(dummy);
    let dummy_ref = GraphNodeRef {
        graph_id: graph.id.clone(),
        node: dummy_index,
    };

    if let Some(in_port) = in_port {
        let dummy_port = push_dummy_port(graph, dummy_index, PortType::Input, PortSide::West);
        graph.layerless_nodes[dummy_index].ports[dummy_port.port].origin_port = Some(in_port);

        let edges = graph.layerless_nodes[in_port.node].ports[in_port.port]
            .incoming_edges
            .clone();
        for edge in edges {
            graph.set_edge_target(edge, dummy_port);
        }
        graph.layerless_nodes[in_port.node].ports[in_port.port].port_dummy =
            Some(dummy_ref.clone());
    }

    if let Some(out_port) = out_port {
        let dummy_port = push_dummy_port(graph, dummy_index, PortType::Output, PortSide::East);
        graph.layerless_nodes[dummy_index].ports[dummy_port.port].origin_port = Some(out_port);

        let edges = graph.layerless_nodes[out_port.node].ports[out_port.port]
            .outgoing_edges
            .clone();
        for edge in edges {
            graph.set_edge_source(edge, dummy_port);
        }
        graph.layerless_nodes[out_port.node].ports[out_port.port].port_dummy = Some(dummy_ref);
    }

    dummy_index
}

fn push_dummy_port(
    graph: &mut LGraph,
    dummy: usize,
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

/// Ports `NorthSouthPortPostprocessor.process`, without the spline branch.
///
/// Takes the dummy nodes out of their layers and gives each edge back to the port it came from.
/// Each edge gets a bend point at the height of the dummy, above or below the node, straight
/// over or under the port.
pub fn postprocess_north_south_ports(graph: &mut LGraph) {
    for layer_index in 0..graph.layers.len() {
        // A copy, because the loop takes nodes out of the layer.
        let node_array = graph.layers[layer_index].nodes.clone();
        for node in node_array {
            if graph.layerless_nodes[node].kind != LNodeKind::NorthSouthPort {
                continue;
            }

            // Whether the dummy has several ports and all of them stand for the same port.
            let ports = &graph.layerless_nodes[node].ports;
            let same_origin_port = ports.len() >= 2
                && ports
                    .ids()
                    .zip(ports.ids().skip(1))
                    .all(|(first, second)| ports[first].origin_port == ports[second].origin_port);

            // Moving an edge to the origin port never changes the dummy's list.
            for position in 0..graph.layerless_nodes[node].ports.len() {
                let Some(port) = graph.layerless_nodes[node].ports.id_at(position) else {
                    continue;
                };
                let dummy_port = PortRef { node, port };
                if !graph.layerless_nodes[node].ports[port]
                    .incoming_edges
                    .is_empty()
                {
                    process_input_port(graph, dummy_port, same_origin_port);
                }
                if !graph.layerless_nodes[node].ports[port]
                    .outgoing_edges
                    .is_empty()
                {
                    process_output_port(graph, dummy_port, same_origin_port);
                }
            }

            graph.remove_node_from_layer(node);
        }
    }
}

/// Ports `NorthSouthPortPostprocessor.processInputPort`: gives the edges that enter the dummy
/// port back to the port the dummy was made for.
///
/// `add_junction_points` is true for a dummy whose ports all stand for one port: its edges meet
/// at the bend point.
fn process_input_port(graph: &mut LGraph, input_port: PortRef, add_junction_points: bool) {
    let Some(origin_port) =
        graph.layerless_nodes[input_port.node].ports[input_port.port].origin_port
    else {
        return;
    };
    let x = absolute_anchor(graph, origin_port).x;
    let y = graph.layerless_nodes[input_port.node].position.y;

    let edges = graph.layerless_nodes[input_port.node].ports[input_port.port]
        .incoming_edges
        .clone();
    for edge in edges {
        graph.set_edge_target(edge, origin_port);
        graph.edges[edge].bend_points.push(LPoint { x, y });
        if add_junction_points {
            graph.edges[edge].junction_points.push(LPoint { x, y });
        }
    }
}

/// Ports `NorthSouthPortPostprocessor.processOutputPort`: gives the edges that leave the dummy
/// port back to the port the dummy was made for.
fn process_output_port(graph: &mut LGraph, output_port: PortRef, add_junction_points: bool) {
    let Some(origin_port) =
        graph.layerless_nodes[output_port.node].ports[output_port.port].origin_port
    else {
        return;
    };
    let x = absolute_anchor(graph, origin_port).x;
    let y = graph.layerless_nodes[output_port.node].position.y;

    let edges = graph.layerless_nodes[output_port.node].ports[output_port.port]
        .outgoing_edges
        .clone();
    for edge in edges {
        graph.set_edge_source(edge, origin_port);
        graph.edges[edge].bend_points.insert(0, LPoint { x, y });
        if add_junction_points {
            graph.edges[edge].junction_points.push(LPoint { x, y });
        }
    }
}

/// `LPort.getAbsoluteAnchor`: the node's position, the port's position and the port's anchor.
fn absolute_anchor(graph: &LGraph, port: PortRef) -> LPoint {
    let node = &graph.layerless_nodes[port.node];
    let data = &node.ports[port.port];
    LPoint {
        x: node.position.x + data.position.x + data.anchor.x,
        y: node.position.y + data.position.y + data.anchor.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LayeredEdge;
    use crate::graph::PortId;
    use crate::options::{ElkDirection, LayeredOptions};

    fn graph() -> LGraph {
        LGraph::new(
            "g",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        )
    }

    /// A node in the given layer with one port per side, in the order given.
    fn add_node(
        graph: &mut LGraph,
        id: &str,
        constraints: PortConstraints,
        sides: &[PortSide],
        layer: usize,
    ) -> usize {
        let index = graph.layerless_nodes.len();
        let mut node = LNode::new(id, 40.0, 20.0, None);
        node.port_constraints = constraints;
        for (position, side) in sides.iter().enumerate() {
            let mut port = LPort::new(format!("{id}.p{position}"), index, PortType::Output);
            port.set_side(*side);
            node.ports.push(port);
        }
        graph.layerless_nodes.push(node);
        graph.set_node_layer(index, layer);
        index
    }

    fn port(node: usize, port: usize) -> PortRef {
        PortRef {
            node,
            port: PortId::new(port),
        }
    }

    fn connect(graph: &mut LGraph, source: PortRef, target: PortRef) -> usize {
        let id = format!("e{}", graph.edges.len());
        graph
            .add_edge(LayeredEdge {
                id,
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
                thickness: 0.0,
                original_opposite_port: None,
                compound_segment: None,
            })
            .expect("both ports exist")
    }

    /// A free-standing node in layer 0 with an east and a west port, to connect the tests' edges.
    fn neighbour(graph: &mut LGraph, id: &str) -> usize {
        add_node(
            graph,
            id,
            PortConstraints::FixedSide,
            &[PortSide::East, PortSide::West],
            0,
        )
    }

    fn layer(graph: &LGraph, index: usize) -> Vec<usize> {
        graph.layers[index].nodes.clone()
    }

    fn dummy_of(graph: &LGraph, node: usize, port: usize) -> usize {
        graph.layerless_nodes[node].ports[PortId::new(port)]
            .port_dummy
            .as_ref()
            .expect("the port has a dummy")
            .node
    }

    // Mutation: leave the edge at the north port instead of moving it to the dummy, or put the
    // dummy below the node.
    #[test]
    fn a_north_port_with_an_incoming_edge_gets_a_dummy_above_its_node() {
        let mut graph = graph();
        let source = neighbour(&mut graph, "s");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North],
            1,
        );
        let edge = connect(&mut graph, port(source, 0), port(node, 0));

        preprocess_north_south_ports(&mut graph);

        let dummy = dummy_of(&graph, node, 0);
        assert_eq!(layer(&graph, 1), vec![dummy, node]);
        let dummy_node = &graph.layerless_nodes[dummy];
        assert_eq!(dummy_node.kind, LNodeKind::NorthSouthPort);
        assert_eq!(dummy_node.port_constraints, PortConstraints::FixedPos);
        assert_eq!(dummy_node.ports.len(), 1);
        assert_eq!(dummy_node.ports[PortId::new(0)].side, PortSide::West);
        assert_eq!(
            dummy_node.ports[PortId::new(0)].origin_port,
            Some(port(node, 0))
        );
        assert_eq!(graph.edges[edge].target, port(dummy, 0));
        assert_eq!(dummy_node.ports[PortId::new(0)].incoming_edges, vec![edge]);
        assert!(
            graph.layerless_nodes[node].ports[PortId::new(0)]
                .incoming_edges
                .is_empty()
        );
        // The dummy and the node form one layout unit. The dummy must stay above the node.
        assert_eq!(dummy_node.in_layer_layout_unit, Some(node));
        assert_eq!(graph.layerless_nodes[node].in_layer_layout_unit, Some(node));
        assert_eq!(dummy_node.in_layer_successor_constraints, vec![node]);
        assert_eq!(
            graph.layerless_nodes[node].barycenter_associates,
            vec![dummy]
        );
    }

    // Mutation: take the edge from the incoming list of the port in place of the outgoing one, or
    // give the dummy a west port for an outgoing edge.
    #[test]
    fn a_north_port_with_an_outgoing_edge_gets_a_dummy_with_an_east_port() {
        let mut graph = graph();
        let target = neighbour(&mut graph, "t");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North],
            1,
        );
        let edge = connect(&mut graph, port(node, 0), port(target, 1));

        preprocess_north_south_ports(&mut graph);

        let dummy = dummy_of(&graph, node, 0);
        let dummy_node = &graph.layerless_nodes[dummy];
        assert_eq!(dummy_node.ports.len(), 1);
        assert_eq!(dummy_node.ports[PortId::new(0)].side, PortSide::East);
        assert_eq!(dummy_node.ports[PortId::new(0)].port_type, PortType::Output);
        assert_eq!(graph.edges[edge].source, port(dummy, 0));
        assert!(
            graph.layerless_nodes[node].ports[PortId::new(0)]
                .outgoing_edges
                .is_empty()
        );
    }

    // Mutation: insert the southern dummies before the node, or make the dummy wait for the node
    // instead of the node for the dummy.
    #[test]
    fn southern_dummies_go_below_the_node_and_the_node_precedes_them() {
        let mut graph = graph();
        let source = neighbour(&mut graph, "s");
        // The ports of the south side are listed from right to left.
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::South, PortSide::South],
            1,
        );
        connect(&mut graph, port(source, 0), port(node, 0));
        connect(&mut graph, port(source, 0), port(node, 1));

        preprocess_north_south_ports(&mut graph);

        // The list puts the port on the right first, and port 0 is the one on the left. The
        // dummies run left to right.
        let first = dummy_of(&graph, node, 0);
        let second = dummy_of(&graph, node, 1);
        assert_eq!(layer(&graph, 1), vec![node, first, second]);
        assert_eq!(
            graph.layerless_nodes[node].in_layer_successor_constraints,
            vec![first, second]
        );
        assert!(
            graph.layerless_nodes[first]
                .in_layer_successor_constraints
                .is_empty()
        );
        assert_eq!(
            graph.layerless_nodes[node].barycenter_associates,
            vec![first, second]
        );
    }

    // Mutation: make one dummy for the two directions of a port, or give the second port of the
    // dummy another origin.
    #[test]
    fn a_port_with_both_directions_gets_one_dummy_with_two_ports() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North],
            1,
        );
        let incoming = connect(&mut graph, port(neighbour, 0), port(node, 0));
        let outgoing = connect(&mut graph, port(node, 0), port(neighbour, 1));

        preprocess_north_south_ports(&mut graph);

        let dummy = dummy_of(&graph, node, 0);
        let ports = &graph.layerless_nodes[dummy].ports;
        assert_eq!(ports.len(), 2);
        assert_eq!(
            (ports[PortId::new(0)].side, ports[PortId::new(1)].side),
            (PortSide::West, PortSide::East)
        );
        assert_eq!(ports[PortId::new(0)].origin_port, Some(port(node, 0)));
        assert_eq!(ports[PortId::new(1)].origin_port, Some(port(node, 0)));
        assert_eq!(graph.edges[incoming].target, port(dummy, 0));
        assert_eq!(graph.edges[outgoing].source, port(dummy, 1));
        assert_eq!(layer(&graph, 1), vec![dummy, node]);
    }

    // Mutation: insert each northern dummy after the one made before it, which keeps the order of
    // creation, or create the dummies for the ports in the order of the port list.
    #[test]
    fn northern_dummies_are_made_for_in_ports_then_out_ports_then_both_and_stack_in_reverse() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        // From left to right: a port with both directions, an out port, an in port. The sort of
        // the port list puts them in the order in, in/out, out.
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedOrder,
            &[PortSide::North, PortSide::North, PortSide::North],
            1,
        );
        connect(&mut graph, port(neighbour, 0), port(node, 0));
        connect(&mut graph, port(node, 0), port(neighbour, 1));
        connect(&mut graph, port(node, 1), port(neighbour, 1));
        connect(&mut graph, port(neighbour, 0), port(node, 2));

        preprocess_north_south_ports(&mut graph);

        // Made in the order in (p2), out (p1), both (p0). Each goes to the node's index, so the
        // last one made ends up first in the layer.
        let both = dummy_of(&graph, node, 0);
        let out = dummy_of(&graph, node, 1);
        let incoming = dummy_of(&graph, node, 2);
        assert_eq!(layer(&graph, 1), vec![both, out, incoming, node]);
        assert_eq!(
            graph.layerless_nodes[node].barycenter_associates,
            vec![incoming, out, both]
        );
    }

    // Mutation: sort the ports of a node whose port order is fixed.
    #[test]
    fn the_port_list_is_sorted_only_when_the_port_order_is_free() {
        for (constraints, expected) in [
            // Free order: in, in/out, out, from west to east.
            (PortConstraints::FixedSide, ["a.p1", "a.p2", "a.p0"]),
            (PortConstraints::FixedOrder, ["a.p0", "a.p1", "a.p2"]),
        ] {
            let mut graph = graph();
            let neighbour = neighbour(&mut graph, "n");
            let node = add_node(
                &mut graph,
                "a",
                constraints,
                &[PortSide::North, PortSide::North, PortSide::North],
                1,
            );
            // p0 is an out port, p1 an in port, p2 has both directions.
            connect(&mut graph, port(node, 0), port(neighbour, 1));
            connect(&mut graph, port(neighbour, 0), port(node, 1));
            connect(&mut graph, port(neighbour, 0), port(node, 2));
            connect(&mut graph, port(node, 2), port(neighbour, 1));

            preprocess_north_south_ports(&mut graph);

            let ids = graph.layerless_nodes[node]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(ids, expected, "{constraints:?}");
        }
    }

    // Mutation: sort the south side in the same direction as the north side.
    #[test]
    fn the_southern_ports_are_sorted_from_east_to_west() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::South, PortSide::South],
            1,
        );
        // p0 is an in port and p1 an out port. The south side runs from east to west, so the
        // out port comes first.
        connect(&mut graph, port(neighbour, 0), port(node, 0));
        connect(&mut graph, port(node, 1), port(neighbour, 1));

        preprocess_north_south_ports(&mut graph);

        let ids = graph.layerless_nodes[node]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["a.p1", "a.p0"]);
    }

    // Mutation: make a dummy for a port without an edge.
    #[test]
    fn a_port_without_an_edge_gets_no_dummy_but_the_node_is_still_a_layout_unit() {
        let mut graph = graph();
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North, PortSide::South],
            0,
        );

        preprocess_north_south_ports(&mut graph);

        assert_eq!(layer(&graph, 0), vec![node]);
        assert!(
            graph.layerless_nodes[node].ports[PortId::new(0)]
                .port_dummy
                .is_none()
        );
        assert!(graph.layerless_nodes[node].barycenter_associates.is_empty());
        assert_eq!(graph.layerless_nodes[node].in_layer_layout_unit, Some(node));
    }

    // Mutation: ignore the option and always add the constraints.
    #[test]
    fn a_port_that_may_switch_sides_adds_no_order_constraint() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North, PortSide::South],
            1,
        );
        graph.layerless_nodes[node].ports[PortId::new(0)].allow_non_flow_ports_to_switch_sides =
            true;
        graph.layerless_nodes[node].ports[PortId::new(1)].allow_non_flow_ports_to_switch_sides =
            true;
        connect(&mut graph, port(neighbour, 0), port(node, 0));
        connect(&mut graph, port(neighbour, 0), port(node, 1));

        preprocess_north_south_ports(&mut graph);

        let north = dummy_of(&graph, node, 0);
        let south = dummy_of(&graph, node, 1);
        assert!(
            graph.layerless_nodes[north]
                .in_layer_successor_constraints
                .is_empty()
        );
        assert!(
            graph.layerless_nodes[node]
                .in_layer_successor_constraints
                .is_empty()
        );
        // The dummies still belong to the node's layout unit and its barycenter.
        assert_eq!(
            graph.layerless_nodes[south].in_layer_layout_unit,
            Some(node)
        );
        assert_eq!(
            graph.layerless_nodes[node].barycenter_associates,
            vec![north, south]
        );
    }

    // Mutation: also process a node whose port sides are free, or a dummy node.
    #[test]
    fn only_regular_nodes_with_fixed_port_sides_get_dummies() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let free = add_node(
            &mut graph,
            "free",
            PortConstraints::Free,
            &[PortSide::North],
            1,
        );
        let long_edge = add_node(
            &mut graph,
            "dummy",
            PortConstraints::FixedSide,
            &[PortSide::North],
            1,
        );
        graph.layerless_nodes[long_edge].kind = LNodeKind::LongEdge;
        connect(&mut graph, port(neighbour, 0), port(free, 0));
        connect(&mut graph, port(neighbour, 0), port(long_edge, 0));

        preprocess_north_south_ports(&mut graph);

        assert_eq!(layer(&graph, 1), vec![free, long_edge]);
        assert_eq!(graph.layerless_nodes.len(), 3);
        assert_eq!(graph.layerless_nodes[free].in_layer_layout_unit, None);
    }

    // Mutation: use the index of the first node for every node of the layer, so that the dummies
    // of the second node go to the wrong place.
    #[test]
    fn dummies_of_a_later_node_follow_the_dummies_of_an_earlier_one() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let first = add_node(
            &mut graph,
            "first",
            PortConstraints::FixedSide,
            &[PortSide::North, PortSide::South],
            1,
        );
        let second = add_node(
            &mut graph,
            "second",
            PortConstraints::FixedSide,
            &[PortSide::North, PortSide::South],
            1,
        );
        for node in [first, second] {
            connect(&mut graph, port(neighbour, 0), port(node, 0));
            connect(&mut graph, port(neighbour, 0), port(node, 1));
        }

        preprocess_north_south_ports(&mut graph);

        let expected = vec![
            dummy_of(&graph, first, 0),
            first,
            dummy_of(&graph, first, 1),
            dummy_of(&graph, second, 0),
            second,
            dummy_of(&graph, second, 1),
        ];
        assert_eq!(layer(&graph, 1), expected);
        for node in [first, second] {
            for port in [0, 1] {
                let dummy = dummy_of(&graph, node, port);
                assert_eq!(graph.layerless_nodes[dummy].layer_index, Some(1));
                assert_eq!(
                    graph.layerless_nodes[dummy].in_layer_layout_unit,
                    Some(node)
                );
            }
        }
    }

    /// Runs the pre-processor on a node with one north port and a neighbour on each side, places
    /// the dummy and the node, and returns the graph, the node and the dummy.
    fn placed_graph(incoming: bool, outgoing: bool) -> (LGraph, usize, usize, Vec<usize>) {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North],
            1,
        );
        let mut edges = Vec::new();
        if incoming {
            edges.push(connect(&mut graph, port(neighbour, 0), port(node, 0)));
        }
        if outgoing {
            edges.push(connect(&mut graph, port(node, 0), port(neighbour, 1)));
        }
        preprocess_north_south_ports(&mut graph);
        let dummy = dummy_of(&graph, node, 0);
        // The node stands at x = 100, the port 7 right of it and 3 wide, its anchor in the
        // middle. The dummy stands at height 12.
        graph.layerless_nodes[node].position = LPoint { x: 100.0, y: 30.0 };
        graph.layerless_nodes[node].ports[PortId::new(0)].position = LPoint { x: 7.0, y: -4.0 };
        graph.layerless_nodes[node].ports[PortId::new(0)].anchor = LPoint { x: 1.5, y: 0.0 };
        graph.layerless_nodes[dummy].position = LPoint { x: 60.0, y: 12.0 };
        (graph, node, dummy, edges)
    }

    // Mutation: use the position of the node instead of its port for the bend point, or forget to
    // take the dummy out of the layer.
    #[test]
    fn an_incoming_edge_goes_back_to_the_port_with_a_bend_point_above_it() {
        let (mut graph, node, dummy, edges) = placed_graph(true, false);
        graph.edges[edges[0]].bend_points = vec![LPoint { x: 50.0, y: 40.0 }];

        postprocess_north_south_ports(&mut graph);

        let edge = &graph.edges[edges[0]];
        assert_eq!(edge.target, port(node, 0));
        // x is the middle of the port, y the height of the dummy; the new point comes last.
        assert_eq!(
            edge.bend_points,
            vec![LPoint { x: 50.0, y: 40.0 }, LPoint { x: 108.5, y: 12.0 }]
        );
        assert!(edge.junction_points.is_empty());
        assert_eq!(
            graph.layerless_nodes[node].ports[PortId::new(0)].incoming_edges,
            edges
        );
        assert_eq!(layer(&graph, 1), vec![node]);
        assert_eq!(graph.layerless_nodes[dummy].layer_index, None);
    }

    // Mutation: add the new bend point last instead of first.
    #[test]
    fn an_outgoing_edge_goes_back_to_the_port_with_a_first_bend_point() {
        let (mut graph, node, _, edges) = placed_graph(false, true);
        graph.edges[edges[0]].bend_points = vec![LPoint { x: 50.0, y: 40.0 }];

        postprocess_north_south_ports(&mut graph);

        let edge = &graph.edges[edges[0]];
        assert_eq!(edge.source, port(node, 0));
        assert_eq!(
            edge.bend_points,
            vec![LPoint { x: 108.5, y: 12.0 }, LPoint { x: 50.0, y: 40.0 }]
        );
        assert!(edge.junction_points.is_empty());
        assert_eq!(
            graph.layerless_nodes[node].ports[PortId::new(0)].outgoing_edges,
            edges
        );
    }

    // Mutation: add junction points for a dummy with one port, or never add them.
    #[test]
    fn a_dummy_for_both_directions_marks_a_junction_point_on_each_edge() {
        let (mut graph, node, _, edges) = placed_graph(true, true);

        postprocess_north_south_ports(&mut graph);

        for edge in &edges {
            assert_eq!(
                graph.edges[*edge].junction_points,
                vec![LPoint { x: 108.5, y: 12.0 }]
            );
            assert_eq!(
                graph.edges[*edge].bend_points,
                vec![LPoint { x: 108.5, y: 12.0 }]
            );
        }
        assert_eq!(graph.edges[edges[0]].target, port(node, 0));
        assert_eq!(graph.edges[edges[1]].source, port(node, 0));
    }

    // Mutation: replace the junction points the router made instead of adding to them.
    #[test]
    fn junction_points_are_added_after_the_ones_the_router_made() {
        let (mut graph, _, _, edges) = placed_graph(true, true);
        graph.edges[edges[0]].junction_points = vec![LPoint { x: 1.0, y: 2.0 }];

        postprocess_north_south_ports(&mut graph);

        assert_eq!(
            graph.edges[edges[0]].junction_points,
            vec![LPoint { x: 1.0, y: 2.0 }, LPoint { x: 108.5, y: 12.0 }]
        );
    }

    // Mutation: add junction points whenever a dummy has two ports, without asking whether they
    // stand for the same port. ELK's "old approach" made such dummies.
    #[test]
    fn a_dummy_whose_ports_stand_for_different_ports_gets_no_junction_points() {
        let (mut graph, node, dummy, edges) = placed_graph(true, false);
        // A second port on the dummy, made for the neighbour's port.
        let neighbour = 0;
        let extra = push_dummy_port(&mut graph, dummy, PortType::Output, PortSide::East);
        graph.layerless_nodes[dummy].ports[extra.port].origin_port = Some(port(neighbour, 0));
        let second = connect(&mut graph, extra, port(neighbour, 1));

        postprocess_north_south_ports(&mut graph);

        assert_eq!(graph.edges[edges[0]].target, port(node, 0));
        assert_eq!(graph.edges[second].source, port(neighbour, 0));
        assert!(graph.edges[edges[0]].junction_points.is_empty());
        assert!(graph.edges[second].junction_points.is_empty());
        assert_eq!(graph.edges[second].bend_points.len(), 1);
    }

    // Mutation: name a new dummy port after `ports.len()` instead of `ports.slot_count()`.
    #[test]
    fn a_new_dummy_port_gets_a_name_no_unlisted_port_has() {
        let (mut graph, _, dummy, _) = placed_graph(true, false);
        let listed = graph.layerless_nodes[dummy].ports.id_at(0).unwrap();
        graph.layerless_nodes[dummy].ports.unlist(&[listed]);

        let extra = push_dummy_port(&mut graph, dummy, PortType::Output, PortSide::East);

        let ports = &graph.layerless_nodes[dummy].ports;
        assert_ne!(ports[extra.port].id, ports[listed].id);
    }

    // Mutation: remove every node of the layer, or only the first dummy.
    #[test]
    fn only_the_dummies_leave_the_layer() {
        let mut graph = graph();
        let neighbour = neighbour(&mut graph, "n");
        let node = add_node(
            &mut graph,
            "a",
            PortConstraints::FixedSide,
            &[PortSide::North, PortSide::South],
            1,
        );
        connect(&mut graph, port(neighbour, 0), port(node, 0));
        connect(&mut graph, port(node, 1), port(neighbour, 1));
        preprocess_north_south_ports(&mut graph);
        assert_eq!(layer(&graph, 1).len(), 3);

        postprocess_north_south_ports(&mut graph);

        assert_eq!(layer(&graph, 1), vec![node]);
        assert_eq!(layer(&graph, 0), vec![neighbour]);
    }
}
