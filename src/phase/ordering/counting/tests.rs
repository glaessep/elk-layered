//! Tests of the classes in this module.

use crate::graph::PortId;
use crate::graph::{LGraph, LNode, LPoint, LayeredEdge, PortType};
use crate::graph::{LNodeKind, NodeId, PortRef, PortSide};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::intermediate::{process_port_sides, sort_port_lists};
use crate::options::{ElkDirection, LayeredOptions, PortConstraints};
use crate::phase::layering::layer_network_simplex;
use crate::phase::ordering::counting::binary_indexed_tree::BinaryIndexedTree;
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;
use crate::phase::ordering::counting::hyperedge_crossings_counter::count_overlapping_areas;

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
        compound_node: false,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
        port_label_placement: Default::default(),
        port_labels_treat_as_group: true,
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
        inside_self_loop: false,
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
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
    graph
}

fn layered_edge(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
    let mut built_edge = LayeredEdge::new(id.to_string(), source, target);
    built_edge.thickness = 1.0;
    built_edge
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
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);

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
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);

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
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);

    let crossing_order = CrossingsCounter::default().count_all_crossings(&graph);
    graph.set_layer_nodes(1, vec![right, left]);
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
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
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
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
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
    crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids(
        &mut graph,
    );
    let order = vec![vec![NodeId::new(0)]];

    let mut counter = CrossingsCounter::default();
    assert_eq!(counter.has_north_south_ports, None);
    counter.count_all_crossings_in_order(&graph, &order);
    assert_eq!(counter.has_north_south_ports, Some(vec![false]));

    // A dummy that shows up later is seen by a new counter at once.
    graph.layerless_nodes[NodeId::new(0)].set_kind(LNodeKind::NorthSouthPort);
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
    assert_eq!(graph.layers()[1].nodes().len(), order.len());
    graph.set_layer_nodes(1, order.to_vec());
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
    let layer = graph.layers()[1].nodes().to_vec();
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
    crate::intermediate::preprocess_north_south_ports(&mut graph);
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
    dummy.set_kind(LNodeKind::LongEdge);
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
    crate::intermediate::preprocess_north_south_ports(&mut graph);
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
    crate::intermediate::preprocess_north_south_ports(&mut graph);
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
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    // The first port of the list is the eastern one, which is port 1. Its dummy is the last in
    // the layer.
    let (east, west) = (dummy_for(&graph, a, 1), dummy_for(&graph, a, 0));
    assert_eq!(graph.layers()[1].nodes(), vec![a, west, east]);
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
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let da = dummy_for(&graph, a, 0);
    let db = dummy_for(&graph, b, 0);
    set_layer_order(&mut graph, &[a, da, db, b]);

    let graph = numbered(&graph);
    let mut counter = CrossingsCounter::default();
    let layer = graph.layers()[1].nodes().to_vec();
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
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
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
    let (graph, [left_upper, left_lower, right_upper, right_lower]) = two_by_two([(0, 1), (1, 0)]);
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
