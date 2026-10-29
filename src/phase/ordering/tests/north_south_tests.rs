use super::*;
use crate::graph::{LPoint, LayeredEdge};
use crate::options::{ElkDirection, LayeredOptions};

fn graph() -> LGraph {
    LGraph::new(
        "g",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
    )
}

fn port(node: NodeId, port: usize) -> PortRef {
    PortRef {
        node,
        port: PortId::new(port),
    }
}

fn add_edge(graph: &mut LGraph, source: PortRef, target: PortRef) {
    let id = format!("e{}", graph.edges.len());
    graph
        .add_edge(LayeredEdge::new(id, source, target))
        .expect("both ports exist");
}

/// A node on `layer` with the given ports, `FIXED_SIDE`.
fn add_node(graph: &mut LGraph, id: &str, layer: usize, sides: &[PortSide]) -> NodeId {
    let index = graph.next_node_id();
    let mut node = LNode::new(id, 40.0, 20.0, None);
    node.port_constraints = PortConstraints::FixedSide;
    graph.layerless_nodes.push(node);
    graph.set_node_layer(index, layer);
    for side in sides {
        let port_type = if *side == PortSide::East {
            PortType::Output
        } else {
            PortType::Input
        };
        graph.add_port(index, port_type, *side, LPoint::default());
    }
    index
}

/// A long edge dummy on the layer, with a neighbour on each side.
fn add_long_edge_dummy(graph: &mut LGraph, layer: usize) -> NodeId {
    let index = graph.next_node_id();
    let mut dummy = LNode::new("long", 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::LongEdge);
    graph.layerless_nodes.push(dummy);
    graph.set_node_layer(index, layer);
    graph.add_port(index, PortType::Input, PortSide::West, LPoint::default());
    graph.add_port(index, PortType::Output, PortSide::East, LPoint::default());
    index
}

fn dummy_of(graph: &LGraph, node: NodeId, port: usize) -> NodeId {
    graph.layerless_nodes[node].ports[PortId::new(port)]
        .port_dummy
        .as_ref()
        .unwrap()
        .node
}

/// Two nodes with one north port each, a neighbour in front. Every node gets the given
/// number of edges for its port. Returns the graph, the nodes and their dummies.
fn two_nodes_with_north_ports(edges: [usize; 2]) -> (LGraph, [NodeId; 2], [NodeId; 2]) {
    let mut graph = graph();
    let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 5]);
    let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::South]);
    let b = add_node(&mut graph, "b", 1, &[PortSide::North, PortSide::South]);
    let mut next = 0;
    for (node, count) in [a, b].into_iter().zip(edges) {
        for _ in 0..count {
            add_edge(&mut graph, port(neighbour, next), port(node, 0));
            next += 1;
        }
    }
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let dummies = [dummy_of(&graph, a, 0), dummy_of(&graph, b, 0)];
    (graph, [a, b], dummies)
}

// Mutation: look for the constraint in the list of the lower node, or ignore the list.
#[test]
fn successor_constraints_prevent_a_switch_in_one_direction_only() {
    let (graph, [a, _], [north, _]) = two_nodes_with_north_ports([1, 1]);
    // The northern dummy must stay above its node.
    assert!(have_successor_constraints(&graph, north, a));
    assert!(!have_successor_constraints(&graph, a, north));
}

// Mutation: leave out the check of the layout units, which keeps a unit together.
#[test]
fn dummies_of_different_nodes_may_not_switch_places() {
    let (graph, [a, b], [north_a, north_b]) = two_nodes_with_north_ports([1, 1]);
    // The southern dummy of the first node stands next to the northern one of the second.
    assert_eq!(graph.layers()[1].nodes().len(), 4);
    assert!(have_layout_unit_constraints(&graph, north_a, north_b));
    assert!(have_layout_unit_constraints(&graph, a, north_b));
    assert!(have_layout_unit_constraints(&graph, a, b));
}

// Mutation: compare the layout units as "both have one" instead of "both are the same".
#[test]
fn dummies_of_one_node_may_switch_places() {
    let mut graph = graph();
    let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 2]);
    let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::North]);
    add_edge(&mut graph, port(neighbour, 0), port(a, 0));
    add_edge(&mut graph, port(neighbour, 1), port(a, 1));
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let (first, second) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));

    // Neither has an edge on a northern or southern side, and both are in the unit of `a`.
    assert!(!have_layout_unit_constraints(&graph, first, second));
    assert!(!have_layout_unit_constraints(&graph, second, first));
}

// Mutation: apply the layout unit rule to long edge dummies.
#[test]
fn a_long_edge_dummy_may_switch_with_any_node() {
    let (mut graph, [a, _], [north_a, _]) = two_nodes_with_north_ports([1, 1]);
    let long = add_long_edge_dummy(&mut graph, 1);
    for node in [a, north_a] {
        assert!(!have_layout_unit_constraints(&graph, node, long));
        assert!(!have_layout_unit_constraints(&graph, long, node));
    }
}

// Mutation: ask only for the edges of the ports, not for the dummies that hold them.
#[test]
fn a_port_with_a_dummy_counts_as_a_port_with_edges() {
    let (graph, [a, _], _) = two_nodes_with_north_ports([1, 1]);
    // The edge of the northern port of `a` lives at the dummy now.
    assert!(
        graph.layerless_nodes[a].ports[PortId::new(0)]
            .incoming_edges()
            .is_empty()
    );
    assert!(has_edges_on_side(&graph, a, PortSide::North));
    // The southern port has no edge and no dummy.
    assert!(!has_edges_on_side(&graph, a, PortSide::South));
}

// Mutation: drop the successor or the layout unit rule from `constraints_prevent_switch`.
#[test]
fn the_switch_decider_applies_all_three_rules() {
    let (mut graph, [a, b], [north_a, north_b]) = two_nodes_with_north_ports([1, 1]);
    let long = add_long_edge_dummy(&mut graph, 1);
    // Two plain nodes that only a successor constraint ties together, and a plain node next
    // to a dummy that has no layout unit and no constraint.
    let first = add_node(&mut graph, "first", 1, &[]);
    let second = add_node(&mut graph, "second", 1, &[]);
    graph
        .tables
        .in_layer
        .row_mut(first)
        .successor_constraints
        .push(second);
    let plain = add_node(&mut graph, "plain", 1, &[]);
    let bare_dummy = add_node(&mut graph, "bare", 1, &[]);
    graph.layerless_nodes[bare_dummy].set_kind(LNodeKind::NorthSouthPort);
    let order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let filler = CrossingMatrixFiller::new(
        order[1].len(),
        CrossMinType::TwoSidedGreedySwitch,
        CrossingCountSide::West,
    );
    let decider = SwitchDecider::new(&graph, &order, 1, filler, false);
    let layer = order[1].clone();
    let index = |node: NodeId| {
        layer
            .iter()
            .position(|candidate| *candidate == node)
            .unwrap()
    };

    // A dummy and its node (rules 1 and 3), dummies of different nodes (rule 2).
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(north_a), index(a)));
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(a), index(north_a)));
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(north_a), index(north_b)));
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(a), index(b)));
    // Only the successor constraint ties these two, and only in one direction.
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(first), index(second)));
    assert!(!decider.constraints_prevent_switch(&graph, &layer, index(second), index(first)));
    // Only the kinds of these two nodes forbid the switch.
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(plain), index(bare_dummy)));
    assert!(decider.constraints_prevent_switch(&graph, &layer, index(bare_dummy), index(plain)));
    // A long edge dummy is free.
    assert!(!decider.constraints_prevent_switch(&graph, &layer, index(a), index(long)));
    assert!(!decider.constraints_prevent_switch(&graph, &layer, index(long), index(north_b)));
}

fn counter_for(graph: &LGraph) -> NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(graph, graph.layers()[1].nodes())
}

/// A node with two northern ports, the first with one edge and the second with two. Returns
/// the graph, the node and the dummies of the first and second port.
fn two_north_ports() -> (LGraph, NodeId, NodeId, NodeId) {
    let mut graph = graph();
    let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 3]);
    let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::North]);
    add_edge(&mut graph, port(neighbour, 0), port(a, 0));
    add_edge(&mut graph, port(neighbour, 1), port(a, 1));
    add_edge(&mut graph, port(neighbour, 2), port(a, 1));
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    // The sweep numbers every port before it counts, and the counter reads the numbers.
    let ports = graph
        .layerless_nodes
        .iter_mut()
        .flat_map(|node| node.ports.slots_mut());
    for (id, port) in ports.enumerate() {
        port.crossing_minimization_id = Some(id);
    }
    let (first, second) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));
    (graph, a, first, second)
}

// Mutation: leave out the count for two dummies of one node, or swap the roles of the dummy
// further from the node and the one closer to it.
#[test]
fn two_dummies_of_one_node_cross_as_often_as_the_edges_of_the_inner_port() {
    let (graph, _, first, second) = two_north_ports();
    let mut counter = counter_for(&graph);

    // The dummy of the left port is further from the node's left corner in this order. The
    // dummy of the right port has two edges.
    counter.count_crossings(&graph, first, second);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (2, 0)
    );
    counter.count_crossings(&graph, second, first);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (0, 2)
    );
}

// Mutation: count the crossings of dummies that belong to different nodes.
#[test]
fn dummies_of_different_nodes_do_not_cross_each_other() {
    let (graph, _, [north_a, north_b]) = two_nodes_with_north_ports([1, 2]);
    let mut counter = counter_for(&graph);
    counter.count_crossings(&graph, north_a, north_b);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (0, 0)
    );
}

// Mutation: swap the northern and southern case, or read the side of the dummy's own port
// instead of the side of the port it was made for.
#[test]
fn a_dummy_crosses_a_long_edge_on_the_side_of_its_port() {
    let mut graph = graph();
    let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 2]);
    let a = add_node(&mut graph, "a", 1, &[PortSide::North, PortSide::South]);
    add_edge(&mut graph, port(neighbour, 0), port(a, 0));
    add_edge(&mut graph, port(neighbour, 1), port(a, 1));
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let (north, south) = (dummy_of(&graph, a, 0), dummy_of(&graph, a, 1));
    let long = add_long_edge_dummy(&mut graph, 1);
    let mut counter = counter_for(&graph);
    let result = |counter: &NorthSouthEdgeNeighbouringNodeCrossingsCounter| {
        (counter.upper_lower_crossings, counter.lower_upper_crossings)
    };

    counter.count_crossings(&graph, north, long);
    assert_eq!(result(&counter), (1, 0));
    counter.count_crossings(&graph, long, north);
    assert_eq!(result(&counter), (0, 1));
    counter.count_crossings(&graph, south, long);
    assert_eq!(result(&counter), (0, 1));
    counter.count_crossings(&graph, long, south);
    assert_eq!(result(&counter), (1, 0));
}

// Mutation: count the ports that have edges instead of the ports that have a dummy.
#[test]
fn a_node_crosses_a_long_edge_once_for_each_port_with_a_dummy() {
    let mut graph = graph();
    let neighbour = add_node(&mut graph, "n", 0, &[PortSide::East; 3]);
    let a = add_node(
        &mut graph,
        "a",
        1,
        &[PortSide::North, PortSide::South, PortSide::South],
    );
    add_edge(&mut graph, port(neighbour, 0), port(a, 0));
    add_edge(&mut graph, port(neighbour, 1), port(a, 1));
    add_edge(&mut graph, port(neighbour, 2), port(a, 2));
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let long = add_long_edge_dummy(&mut graph, 1);
    let mut counter = counter_for(&graph);

    // The node above the long edge: the long edge passes the southern ports' dummies.
    counter.count_crossings(&graph, a, long);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (2, 1)
    );
    counter.count_crossings(&graph, long, a);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (1, 2)
    );
}

// Mutation: collect the layout units from the layers in reverse order.
#[test]
fn the_heuristic_keeps_the_layout_units_in_the_order_of_its_start() {
    let (graph, [a, _], [north_a, _]) = two_nodes_with_north_ports([1, 1]);
    let order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let distributor = BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    let heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), distributor);
    // The dummy is above the node in the layer, so it comes first.
    assert_eq!(heuristic.layout_units[&a], vec![north_a, a]);
}
