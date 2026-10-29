//! Tests of `NorthSouthPortPreprocessor` and `NorthSouthPortPostprocessor`.

use crate::graph::EdgeId;
use crate::graph::LayeredEdge;
use crate::graph::PortId;
use crate::graph::{LGraph, LNode, LNodeKind, LPoint, LPort, NodeId, PortRef, PortSide, PortType};
use crate::intermediate::north_south_port_postprocessor::postprocess_north_south_ports;
use crate::intermediate::north_south_port_preprocessor::{
    preprocess_north_south_ports, push_dummy_port,
};
use crate::options::PortConstraints;
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
) -> NodeId {
    let index = graph.next_node_id();
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

fn port(node: NodeId, port: usize) -> PortRef {
    PortRef {
        node,
        port: PortId::new(port),
    }
}

fn connect(graph: &mut LGraph, source: PortRef, target: PortRef) -> EdgeId {
    let id = format!("e{}", graph.edges.len());
    graph
        .add_edge(LayeredEdge::new(id, source, target))
        .expect("both ports exist")
}

/// A free-standing node in layer 0 with an east and a west port, to connect the tests' edges.
fn neighbour(graph: &mut LGraph, id: &str) -> NodeId {
    add_node(
        graph,
        id,
        PortConstraints::FixedSide,
        &[PortSide::East, PortSide::West],
        0,
    )
}

fn layer(graph: &LGraph, index: usize) -> Vec<NodeId> {
    graph.layers()[index].nodes().to_vec()
}

fn dummy_of(graph: &LGraph, node: NodeId, port: usize) -> NodeId {
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
    assert_eq!(dummy_node.kind(), LNodeKind::NorthSouthPort);
    assert_eq!(dummy_node.port_constraints, PortConstraints::FixedPos);
    assert_eq!(dummy_node.ports.len(), 1);
    assert_eq!(dummy_node.ports[PortId::new(0)].side, PortSide::West);
    assert_eq!(
        dummy_node.ports[PortId::new(0)].origin_port,
        Some(port(node, 0))
    );
    assert_eq!(graph.edges[edge].target(), port(dummy, 0));
    assert_eq!(
        dummy_node.ports[PortId::new(0)].incoming_edges(),
        vec![edge]
    );
    assert!(
        graph.layerless_nodes[node].ports[PortId::new(0)]
            .incoming_edges()
            .is_empty()
    );
    // The dummy and the node form one layout unit. The dummy must stay above the node.
    assert_eq!(graph.in_layer_layout_unit(dummy), Some(node));
    assert_eq!(graph.in_layer_layout_unit(node), Some(node));
    assert_eq!(graph.in_layer_successor_constraints(dummy), vec![node]);
    assert_eq!(graph.barycenter_associates(node), vec![dummy]);
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
    assert_eq!(graph.edges[edge].source(), port(dummy, 0));
    assert!(
        graph.layerless_nodes[node].ports[PortId::new(0)]
            .outgoing_edges()
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
        graph.in_layer_successor_constraints(node),
        vec![first, second]
    );
    assert!(graph.in_layer_successor_constraints(first).is_empty());
    assert_eq!(graph.barycenter_associates(node), vec![first, second]);
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
    assert_eq!(graph.edges[incoming].target(), port(dummy, 0));
    assert_eq!(graph.edges[outgoing].source(), port(dummy, 1));
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
    assert_eq!(graph.barycenter_associates(node), vec![incoming, out, both]);
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
    assert!(graph.barycenter_associates(node).is_empty());
    assert_eq!(graph.in_layer_layout_unit(node), Some(node));
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
    graph.layerless_nodes[node].ports[PortId::new(0)].allow_non_flow_ports_to_switch_sides = true;
    graph.layerless_nodes[node].ports[PortId::new(1)].allow_non_flow_ports_to_switch_sides = true;
    connect(&mut graph, port(neighbour, 0), port(node, 0));
    connect(&mut graph, port(neighbour, 0), port(node, 1));

    preprocess_north_south_ports(&mut graph);

    let north = dummy_of(&graph, node, 0);
    let south = dummy_of(&graph, node, 1);
    assert!(graph.in_layer_successor_constraints(north).is_empty());
    assert!(graph.in_layer_successor_constraints(node).is_empty());
    // The dummies still belong to the node's layout unit and its barycenter.
    assert_eq!(graph.in_layer_layout_unit(south), Some(node));
    assert_eq!(graph.barycenter_associates(node), vec![north, south]);
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
    graph.layerless_nodes[long_edge].set_kind(LNodeKind::LongEdge);
    connect(&mut graph, port(neighbour, 0), port(free, 0));
    connect(&mut graph, port(neighbour, 0), port(long_edge, 0));

    preprocess_north_south_ports(&mut graph);

    assert_eq!(layer(&graph, 1), vec![free, long_edge]);
    assert_eq!(graph.layerless_nodes.len(), 3);
    assert_eq!(graph.in_layer_layout_unit(free), None);
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
            assert_eq!(graph.layerless_nodes[dummy].layer_index(), Some(1));
            assert_eq!(graph.in_layer_layout_unit(dummy), Some(node));
        }
    }
}

/// Runs the pre-processor on a node with one north port and a neighbour on each side, places
/// the dummy and the node, and returns the graph, the node and the dummy.
fn placed_graph(incoming: bool, outgoing: bool) -> (LGraph, NodeId, NodeId, Vec<EdgeId>) {
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
    assert_eq!(edge.target(), port(node, 0));
    // x is the middle of the port, y the height of the dummy; the new point comes last.
    assert_eq!(
        edge.bend_points,
        vec![LPoint { x: 50.0, y: 40.0 }, LPoint { x: 108.5, y: 12.0 }]
    );
    assert!(edge.junction_points.is_empty());
    assert_eq!(
        graph.layerless_nodes[node].ports[PortId::new(0)].incoming_edges(),
        edges
    );
    assert_eq!(layer(&graph, 1), vec![node]);
    assert_eq!(graph.layerless_nodes[dummy].layer_index(), None);
}

// Mutation: add the new bend point last instead of first.
#[test]
fn an_outgoing_edge_goes_back_to_the_port_with_a_first_bend_point() {
    let (mut graph, node, _, edges) = placed_graph(false, true);
    graph.edges[edges[0]].bend_points = vec![LPoint { x: 50.0, y: 40.0 }];

    postprocess_north_south_ports(&mut graph);

    let edge = &graph.edges[edges[0]];
    assert_eq!(edge.source(), port(node, 0));
    assert_eq!(
        edge.bend_points,
        vec![LPoint { x: 108.5, y: 12.0 }, LPoint { x: 50.0, y: 40.0 }]
    );
    assert!(edge.junction_points.is_empty());
    assert_eq!(
        graph.layerless_nodes[node].ports[PortId::new(0)].outgoing_edges(),
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
    assert_eq!(graph.edges[edges[0]].target(), port(node, 0));
    assert_eq!(graph.edges[edges[1]].source(), port(node, 0));
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
    let neighbour = NodeId::new(0);
    let extra = push_dummy_port(&mut graph, dummy, PortType::Output, PortSide::East);
    graph.layerless_nodes[dummy].ports[extra.port].origin_port = Some(port(neighbour, 0));
    let second = connect(&mut graph, extra, port(neighbour, 1));

    postprocess_north_south_ports(&mut graph);

    assert_eq!(graph.edges[edges[0]].target(), port(node, 0));
    assert_eq!(graph.edges[second].source(), port(neighbour, 0));
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
