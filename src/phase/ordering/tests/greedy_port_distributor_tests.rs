use super::*;
use crate::graph::{GraphNodeRef, LPoint, LayeredEdge};
use crate::options::{ElkDirection, LayeredOptions};

fn graph(id: &str) -> LGraph {
    LGraph::new(
        id,
        LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
    )
}

// Mutation: draw a boolean for the two-sided type too, or draw none for the one-sided type.
// The shared `Random` then sits one step off for the next connected component.
#[test]
fn creating_a_port_distributor_draws_as_isweep_port_distributor_create_does() {
    let order = vec![vec![NodeId::new(0)]];
    let next = |cross_min_type| {
        let mut random = JavaRandom::new(7);
        let distributor = PortDistributor::create(cross_min_type, &mut random, &order);
        (distributor, random.next_long())
    };
    let untouched = JavaRandom::new(7).next_long();
    let mut after_one_draw = JavaRandom::new(7);
    after_one_draw.next_bool();
    let after_one_draw = after_one_draw.next_long();

    let (two_sided, seen) = next(CrossMinType::TwoSidedGreedySwitch);
    assert!(matches!(two_sided, PortDistributor::Greedy(_)));
    assert_eq!(seen, untouched);
    for cross_min_type in [CrossMinType::OneSidedGreedySwitch, CrossMinType::Barycenter] {
        let (distributor, seen) = next(cross_min_type);
        assert!(matches!(distributor, PortDistributor::Barycenter(_)));
        assert_eq!(seen, after_one_draw);
    }
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

/// A node on `layer` with one port per side given, in the given order of the port list.
fn add_node(
    graph: &mut LGraph,
    id: &str,
    layer: usize,
    constraints: PortConstraints,
    sides: &[PortSide],
) -> NodeId {
    let index = graph.next_node_id();
    let mut node = LNode::new(id, 40.0, 20.0, None);
    node.port_constraints = constraints;
    graph.layerless_nodes.push(node);
    graph.set_node_layer(index, layer);
    for (position, side) in sides.iter().enumerate() {
        let port_type = if matches!(side, PortSide::East) {
            PortType::Output
        } else {
            PortType::Input
        };
        let added = graph
            .add_port(index, port_type, *side, LPoint::default())
            .unwrap();
        graph.layerless_nodes[index].ports[added.port].id = format!("{id}.p{position}");
    }
    index
}

fn order(graph: &LGraph) -> Vec<Vec<NodeId>> {
    graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect()
}

fn port_ids(graph: &LGraph, node: NodeId) -> Vec<String> {
    graph.layerless_nodes[node]
        .ports
        .iter()
        .map(|port| port.id.clone())
        .collect()
}

/// Two nodes `top` and `bottom` in layer 0, one node `a` in layer 1. The ports of `a` are the
/// west ports `a.p0` and `a.p1`, listed from bottom to top. `top` connects to `a.p0`, so to
/// the lower port, and `bottom` to `a.p1`: the edges cross.
fn crossed(constraints: PortConstraints) -> (LGraph, NodeId) {
    let mut graph = graph("g");
    let top = add_node(
        &mut graph,
        "top",
        0,
        PortConstraints::FixedSide,
        &[PortSide::East],
    );
    let bottom = add_node(
        &mut graph,
        "bottom",
        0,
        PortConstraints::FixedSide,
        &[PortSide::East],
    );
    let a = add_node(
        &mut graph,
        "a",
        1,
        constraints,
        &[PortSide::West, PortSide::West],
    );
    add_edge(&mut graph, port(top, 0), port(a, 0));
    add_edge(&mut graph, port(bottom, 0), port(a, 1));
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
    (graph, a)
}

// Mutation: never switch, or switch when the crossings would stay the same.
#[test]
fn crossed_ports_on_the_side_of_the_sweep_switch_places() {
    let (mut graph, a) = crossed(PortConstraints::FixedSide);
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();

    let improved = distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        true,
    );

    assert!(improved);
    // The list runs from bottom to top, so the upper edge now ends at the second port.
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
    // The ports moved in the list. The edges did not move: each still ends at its port.
    let top_edge = &graph.edges[EdgeId::new(0)];
    let bottom_edge = &graph.edges[EdgeId::new(1)];
    assert_eq!(top_edge.target(), port(a, 0));
    assert_eq!(bottom_edge.target(), port(a, 1));
    assert_eq!(
        graph.layerless_nodes[a].ports[PortId::new(0)].incoming_edges(),
        vec![EdgeId::new(0)]
    );
    assert_eq!(
        graph.layerless_nodes[a].ports.position(PortId::new(0)),
        Some(1)
    );
}

// Mutation: switch ports that do not cross.
#[test]
fn ports_that_do_not_cross_stay_where_they_are() {
    let (mut graph, a) = crossed(PortConstraints::FixedSide);
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();
    distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        true,
    );
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);

    // A second pass finds nothing to improve.
    let improved = distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        true,
    );

    assert!(!improved);
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
}

// Mutation: ignore the port constraints of the node.
#[test]
fn a_node_with_a_fixed_port_order_keeps_its_ports() {
    for constraints in [PortConstraints::FixedOrder, PortConstraints::FixedPos] {
        let (mut graph, a) = crossed(constraints);
        let order = order(&graph);
        let mut distributor = GreedyPortDistributor::default();

        let improved = distributor.distribute_ports_while_sweeping(
            &mut graph,
            crate::graph::OtherGraphs::none(),
            &order,
            1,
            true,
        );

        assert!(!improved, "{constraints:?}");
        assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"], "{constraints:?}");
    }
}

// Mutation: distribute the ports of the west side in a backward sweep.
#[test]
fn only_the_side_that_faces_the_sweep_is_distributed() {
    let (mut graph, a) = crossed(PortConstraints::FixedSide);
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();

    let improved = distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        false,
    );

    assert!(!improved);
    assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"]);
}

// Mutation: read the ports of an east side from the other end of the list.
#[test]
fn crossed_ports_on_the_east_side_switch_places_in_a_backward_sweep() {
    let mut graph = graph("g");
    let a = add_node(
        &mut graph,
        "a",
        0,
        PortConstraints::FixedSide,
        &[PortSide::East, PortSide::East],
    );
    let top = add_node(
        &mut graph,
        "top",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West],
    );
    let bottom = add_node(
        &mut graph,
        "bottom",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West],
    );
    // The list of the east side runs from top to bottom. `a.p0` is the upper port and goes
    // to the lower node.
    add_edge(&mut graph, port(a, 0), port(bottom, 0));
    add_edge(&mut graph, port(a, 1), port(top, 0));
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();

    let improved = distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        0,
        false,
    );

    assert!(improved);
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
}

// Mutation: switch ports whose counts are equal. The loop would then never end.
#[test]
fn ports_without_edges_stay_where_they_are() {
    let mut graph = graph("g");
    let a = add_node(
        &mut graph,
        "a",
        0,
        PortConstraints::FixedSide,
        &[PortSide::West, PortSide::West],
    );
    let b = add_node(
        &mut graph,
        "b",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West],
    );
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();

    let improved = distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        0,
        true,
    );

    assert!(!improved);
    assert_eq!(port_ids(&graph, a), ["a.p0", "a.p1"]);
    assert_eq!(port_ids(&graph, b), ["b.p0"]);
}

// Mutation: leave the positions of the counter at the slot of a port that moved.
#[test]
fn the_positions_of_the_counter_move_with_the_ports() {
    let (mut graph, a) = crossed(PortConstraints::FixedSide);
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();
    distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        true,
    );

    // The west ports of layer 1 are numbered after the east ports of layer 0, so they get 2
    // and 3. The port `a.p0` had position 2 before the switch. It is the second in the list
    // now, and the switch gave it the position of the other one.
    let counter = &distributor.crossings_counter;
    assert_eq!(graph.layerless_nodes[a].ports[PortId::new(0)].id, "a.p0");
    assert_eq!(counter.position_of(&graph, port(a, 1)), 2);
    assert_eq!(counter.position_of(&graph, port(a, 0)), 3);
}

// Mutation: leave the positions of the counter at the slot of a port that moved, so that the
// next count reads the position of the wrong port.
#[test]
fn the_counter_follows_the_ports_that_moved() {
    let (mut graph, a) = crossed(PortConstraints::FixedSide);
    let order = order(&graph);
    let mut distributor = GreedyPortDistributor::default();
    distributor.distribute_ports_while_sweeping(
        &mut graph,
        crate::graph::OtherGraphs::none(),
        &order,
        1,
        true,
    );

    // The port that was first (port 0) is second now. In the numbering of the west side of
    // layer 1, a west port further down the list is further up, so the first port of the list
    // (port 1) is the lower one.
    distributor
        .crossings_counter
        .init_for_counting_between(&graph, &order[0], &order[1]);
    let counts = distributor
        .crossings_counter
        .count_crossings_between_ports_in_both_orders(&graph, port(a, 1), port(a, 0));
    // The edges are parallel now. Switching the two ports makes them cross.
    assert_eq!(counts, (0.0, 1.0));
}

/// A node `x` in layer 0 whose two east ports cannot move, and a node `a` in layer 1 with
/// two west ports that can. The edges cross until the ports of `a` switch places.
fn crossed_between_two_nodes() -> (LGraph, NodeId) {
    let mut graph = graph("g");
    let x = add_node(
        &mut graph,
        "x",
        0,
        PortConstraints::FixedOrder,
        &[PortSide::East, PortSide::East],
    );
    let a = add_node(
        &mut graph,
        "a",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West, PortSide::West],
    );
    // `x.p0` is the upper port of `x`, and `a.p0` is the lower port of `a`.
    add_edge(&mut graph, port(x, 0), port(a, 0));
    add_edge(&mut graph, port(x, 1), port(a, 1));
    (graph, a)
}

/// A seed for which the first sweep of a greedy switch runs forward: the minimizer draws one
/// long, the one-sided type draws a boolean for its port distributor, and then the direction
/// comes. A backward first sweep would distribute the east ports only, and these cannot move.
fn seed_for_a_forward_first_sweep(cross_min_type: CrossMinType) -> i64 {
    (0..)
        .find(|seed| {
            let mut random = JavaRandom::new(*seed);
            random.next_long();
            if cross_min_type == CrossMinType::OneSidedGreedySwitch {
                random.next_bool();
            }
            random.next_bool()
        })
        .unwrap()
}

// Mutation: do not call the port distributor in the flat sweep of the two-sided greedy switch.
#[test]
fn the_flat_two_sided_sweep_distributes_ports() {
    let (mut graph, a) = crossed_between_two_nodes();
    graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(
        CrossMinType::TwoSidedGreedySwitch,
    ));
    sweep_alone(&mut graph, CrossMinType::TwoSidedGreedySwitch);
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
}

// Mutation: do not call the barycenter port distributor in the flat sweep of the one-sided
// greedy switch, as the port did before. ELK's `ISweepPortDistributor.create` makes one for
// every type but the two-sided greedy switch.
#[test]
fn the_flat_one_sided_sweep_distributes_ports() {
    let (mut graph, a) = crossed_between_two_nodes();
    graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(
        CrossMinType::OneSidedGreedySwitch,
    ));
    sweep_alone(&mut graph, CrossMinType::OneSidedGreedySwitch);
    assert_eq!(port_ids(&graph, a), ["a.p1", "a.p0"]);
}

// Mutation: do not call the port distributor in the hierarchical sweep of the two-sided
// greedy switch, or do not call the barycenter one in the hierarchical sweep of the one-sided
// type.
#[test]
fn the_hierarchical_greedy_sweeps_distribute_ports() {
    for cross_min_type in [
        CrossMinType::TwoSidedGreedySwitch,
        CrossMinType::OneSidedGreedySwitch,
    ] {
        let (mut graph, a) = crossed_between_two_nodes();
        graph.random = JavaRandom::new(seed_for_a_forward_first_sweep(cross_min_type));
        let mut graphs = GraphArena::new(graph);
        minimize_crossings_layer_sweep(&mut graphs, GraphId::ROOT, true, cross_min_type);
        assert_eq!(
            port_ids(graphs.root(), a),
            ["a.p1", "a.p0"],
            "{cross_min_type:?}"
        );
    }
}

/// A parent node `p` in layer 0 with two west ports and a nested graph. The nested graph has
/// the dummies of the ports in layer 0 and two nodes in layer 1, joined so that the edges
/// cross when the ports stay as they are. The parent has no edge outside.
fn parent_with_crossing_inside() -> (GraphArena, NodeId) {
    let mut inner = graph("inner");
    let first = inner.next_node_id();
    inner.layerless_nodes.push(LNode::new("d0", 1.0, 1.0, None));
    inner.layerless_nodes[first].set_kind(LNodeKind::ExternalPort);
    inner.set_node_layer(first, 0);
    inner.add_port(first, PortType::Output, PortSide::East, LPoint::default());
    let second = inner.next_node_id();
    inner.layerless_nodes.push(LNode::new("d1", 1.0, 1.0, None));
    inner.layerless_nodes[second].set_kind(LNodeKind::ExternalPort);
    inner.set_node_layer(second, 0);
    inner.add_port(second, PortType::Output, PortSide::East, LPoint::default());
    let c0 = add_node(
        &mut inner,
        "c0",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West],
    );
    let c1 = add_node(
        &mut inner,
        "c1",
        1,
        PortConstraints::FixedSide,
        &[PortSide::West],
    );
    inner.id = GraphId::new(1);
    let inner_id = inner.id;
    // `d1` is below `d0`. `d1` goes to the lower node and `d0` to the upper one.
    add_edge(&mut inner, port(second, 0), port(c1, 0));
    add_edge(&mut inner, port(first, 0), port(c0, 0));
    // The ports of the parent, in the order of the list, stand for d0 and d1.
    let mut outer = graph("outer");
    let parent = add_node(
        &mut outer,
        "p",
        0,
        PortConstraints::FixedSide,
        &[PortSide::West, PortSide::West],
    );
    for (port_index, dummy) in [first, second].into_iter().enumerate() {
        outer.layerless_nodes[parent].ports[PortId::new(port_index)].port_dummy =
            Some(GraphNodeRef {
                graph_id: inner_id,
                node: dummy,
            });
    }
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut outer,
    );
    let mut graphs = GraphArena::new(outer);
    let nested = graphs.nest(GraphId::ROOT, parent, inner);
    assert_eq!(
        nested, inner_id,
        "the dummies name the graph inside the node"
    );
    (graphs, parent)
}

// Mutation: leave out the crossings inside the node.
#[test]
fn the_crossings_inside_a_node_with_children_decide_the_order_of_its_ports() {
    let (mut graphs, parent) = parent_with_crossing_inside();
    let order = order(graphs.root());
    let mut distributor = GreedyPortDistributor::default();

    let (graph, others) = graphs.split_mut(GraphId::ROOT);
    let improved = distributor.distribute_ports_while_sweeping(graph, others, &order, 0, true);

    assert!(improved);
    assert_eq!(port_ids(graph, parent), ["p.p1", "p.p0"]);
    // The dummies keep their ports.
    let dummies = graph.layerless_nodes[parent]
        .ports
        .iter()
        .map(|port| port.port_dummy.as_ref().unwrap().node)
        .collect::<Vec<_>>();
    assert_eq!(dummies, vec![NodeId::new(1), NodeId::new(0)]);
}
