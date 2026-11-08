//! Tests of the three partition processors.

use crate::graph::{EdgeId, GraphArena, LayeredEdge, PortId, PortSide, PortType};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::intermediate::{
    IntermediateError, midprocess_partitions, postprocess_partitions, preprocess_partitions,
};
use crate::options::{ElkDirection, LayeredOptions};

fn node(id: &str, partition: Option<i32>) -> ElkInputNode {
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
        partition,
    }
}

fn edge(id: &str, source: &str, target: &str, priority_direction: i32) -> ElkInputEdge {
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
        priority_direction,
        priority_shortness: 0,
        priority_straightness: 0,
        container: None,
    }
}

fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> GraphArena {
    import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap()
}

/// The ids of the nodes the edge connects, as `(source, target)`.
fn ends(graph: &GraphArena, edge: EdgeId) -> (String, String) {
    let edge = &graph.edges[edge];
    (
        graph.layerless_nodes[edge.source().node].id.clone(),
        graph.layerless_nodes[edge.target().node].id.clone(),
    )
}

fn pair(source: &str, target: &str) -> (String, String) {
    (source.to_string(), target.to_string())
}

// Mutation: in `must_be_reversed`, compare with `source < target`.
#[test]
fn an_edge_from_a_higher_partition_to_a_lower_one_is_reversed() {
    let mut graph = graph(
        vec![node("a", Some(1)), node("b", Some(0))],
        vec![edge("e", "a", "b", 0)],
    );

    preprocess_partitions(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].reversed);
    assert_eq!(ends(&graph, EdgeId::new(0)), pair("b", "a"));
}

// Mutation: in `must_be_reversed`, compare with `source >= target`.
#[test]
fn an_edge_that_runs_forward_or_inside_one_partition_stays() {
    let mut graph = graph(
        vec![
            node("a", Some(0)),
            node("b", Some(1)),
            node("c", Some(1)),
            node("d", Some(-5)),
        ],
        vec![
            edge("forward", "a", "b", 0),
            edge("inside", "b", "c", 0),
            // A self loop has the same partition at both ends.
            edge("loop", "b", "b", 0),
            // -5 is below 0, so the edge to it is backward: the sign must count.
            edge("negative", "d", "a", 0),
        ],
    );

    preprocess_partitions(&mut graph);

    for index in 0..4 {
        assert!(!graph.edges[EdgeId::new(index)].reversed, "edge {index}");
        assert_eq!(graph.edges[EdgeId::new(index)].priority_direction, 0);
    }
}

// Mutation: in `must_be_reversed`, read a missing partition as 0.
#[test]
fn an_edge_at_a_node_without_a_partition_stays() {
    let mut graph = graph(
        vec![node("a", Some(1)), node("x", None), node("b", Some(-1))],
        vec![
            edge("to-x", "a", "x", 0),
            edge("from-x", "x", "b", 0),
            // `x` sits at 0 if it is read as partition 0. Then `a` to `x` runs from 1 to 0.
        ],
    );

    preprocess_partitions(&mut graph);

    assert!(!graph.edges[EdgeId::new(0)].reversed);
    assert!(!graph.edges[EdgeId::new(1)].reversed);
}

// Mutation: in `preprocess_partitions`, skip a node whose partition is 0 as well as one with none.
#[test]
fn a_node_in_partition_zero_has_a_partition() {
    let mut graph = graph(
        vec![node("a", Some(0)), node("b", Some(-1))],
        vec![edge("e", "a", "b", 0)],
    );

    preprocess_partitions(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].reversed);
}

// Mutation: in `reverse`, keep the user's priority when it is set instead of adding to it.
#[test]
fn a_reversed_edge_gets_the_priority_1000_on_top_of_the_users() {
    let mut graph = graph(
        vec![node("a", Some(1)), node("b", Some(0))],
        vec![
            edge("none", "a", "b", 0),
            edge("some", "a", "b", 7),
            edge("negative", "a", "b", -5),
        ],
    );

    preprocess_partitions(&mut graph);

    let priorities: Vec<i32> = (0..3)
        .map(|index| graph.edges[EdgeId::new(index)].priority_direction)
        .collect();
    assert_eq!(priorities, [1000, 1007, 995]);
}

// Mutation: in `reverse`, add with `+` instead of `wrapping_add`. Debug builds then panic.
#[test]
fn the_priority_wraps_around_like_a_java_int() {
    let mut graph = graph(
        vec![node("a", Some(1)), node("b", Some(0))],
        vec![edge("e", "a", "b", i32::MAX)],
    );

    preprocess_partitions(&mut graph);

    assert_eq!(
        graph.edges[EdgeId::new(0)].priority_direction,
        i32::MIN + 999
    );
}

// Mutation: in `midprocess_partitions`, connect only the first pair of partitions (`.take(1)`).
#[test]
fn a_partition_is_connected_to_the_next_one_that_exists() {
    let mut graph = graph(
        vec![
            node("a", Some(0)),
            node("b", Some(0)),
            node("c", Some(5)),
            node("d", Some(9)),
            node("x", None),
            node("n", Some(-3)),
        ],
        vec![],
    );

    midprocess_partitions(&mut graph);

    let dummies: Vec<_> = (0..graph.edges.len())
        .map(|index| ends(&graph, EdgeId::new(index)))
        .collect();
    assert_eq!(
        dummies,
        [
            pair("n", "a"),
            pair("n", "b"),
            pair("a", "c"),
            pair("b", "c"),
            pair("c", "d"),
        ]
    );
    assert!(graph.edges.iter().all(|edge| edge.partition_dummy));
    // The node without a partition takes no part.
    let x = graph.node_named("x");
    assert!(graph.layerless_nodes[x].ports.is_empty());
}

// Mutation: in `connect_nodes`, give the dummy ports the other sides.
#[test]
fn a_dummy_port_sits_east_of_the_lower_partition_and_west_of_the_higher_one() {
    let mut graph = graph(vec![node("a", Some(0)), node("b", Some(1))], vec![]);

    midprocess_partitions(&mut graph);

    let a = graph.node_named("a");
    let b = graph.node_named("b");
    let source = graph.layerless_nodes[a].ports.first().unwrap();
    let target = graph.layerless_nodes[b].ports.first().unwrap();
    assert_eq!(
        (graph.layerless_nodes[a].ports.len(), source.side),
        (1, PortSide::East)
    );
    assert_eq!(
        (graph.layerless_nodes[b].ports.len(), target.side),
        (1, PortSide::West)
    );
    assert!(source.partition_dummy && target.partition_dummy);
}

// Mutation: in `connect_nodes`, make a new east port for each edge instead of one for each node.
#[test]
fn every_edge_gets_a_port_of_its_own_at_both_ends() {
    let mut graph = graph(
        vec![
            node("a", Some(0)),
            node("b", Some(0)),
            node("c", Some(1)),
            node("d", Some(1)),
        ],
        vec![],
    );

    midprocess_partitions(&mut graph);

    // One east port per node of partition 0 (it serves both edges that leave it), and one west
    // port per edge at each node of partition 1: two, because both nodes of partition 0 join it.
    let counts: Vec<usize> = ["a", "b", "c", "d"]
        .iter()
        .map(|id| graph.layerless_nodes[graph.node_named(id)].ports.len())
        .collect();
    assert_eq!(counts, [1, 1, 2, 2]);
}

// Mutation: in `midprocess_partitions`, put each node at the front of its partition's list.
#[test]
fn the_nodes_of_a_partition_keep_the_order_of_the_graph() {
    let mut graph = graph(
        vec![
            node("b", Some(0)),
            node("a", Some(0)),
            node("d", Some(1)),
            node("c", Some(1)),
        ],
        vec![],
    );

    midprocess_partitions(&mut graph);

    let dummies: Vec<_> = (0..graph.edges.len())
        .map(|index| ends(&graph, EdgeId::new(index)))
        .collect();
    // Each node of the lower partition connects to all of the higher one, in the graph's order.
    assert_eq!(
        dummies,
        [
            pair("b", "d"),
            pair("b", "c"),
            pair("a", "d"),
            pair("a", "c")
        ]
    );
}

// Mutation: in `midprocess_partitions`, drop the check of the hidden mask.
#[test]
fn a_node_that_the_layer_constraints_hid_gets_no_dummy() {
    let mut graph = graph(vec![node("a", Some(0)), node("b", Some(1))], vec![]);
    let b = graph.node_named("b");
    graph.hidden_nodes.push(b);

    midprocess_partitions(&mut graph);

    assert!(graph.edges.is_empty());
    assert!(graph.layerless_nodes[b].ports.is_empty());
}

// Mutation: in `midprocess_partitions`, connect each partition to itself.
#[test]
fn one_partition_or_none_adds_nothing() {
    for nodes in [
        vec![node("a", None), node("b", None)],
        vec![node("a", Some(2)), node("b", Some(2)), node("x", None)],
        vec![],
    ] {
        let mut graph = graph(nodes, vec![]);

        midprocess_partitions(&mut graph);

        assert!(graph.edges.is_empty());
        assert!(
            graph
                .layerless_nodes
                .iter()
                .all(|node| node.ports.is_empty())
        );
    }
}

// Mutation: in `postprocess_partitions`, leave the ports listed.
#[test]
fn the_postprocessor_takes_back_what_the_midprocessor_added() {
    let mut graph = graph(
        vec![node("a", Some(0)), node("b", Some(1)), node("c", Some(1))],
        vec![edge("real", "a", "b", 0), edge("other", "c", "a", 0)],
    );
    preprocess_partitions(&mut graph);
    let before = graph.clone();
    let real_edges = graph.edges.len();

    midprocess_partitions(&mut graph);
    assert!(graph.edges.len() > real_edges);
    postprocess_partitions(&mut graph).unwrap();

    assert_eq!(graph.edges, before.edges);
    for (id, node) in graph.nodes_with_ids() {
        let want: Vec<_> = before.layerless_nodes[id].ports.iter().collect();
        let got: Vec<_> = node.ports.iter().collect();
        assert_eq!(got, want, "ports of {}", node.id);
    }
}

// Mutation: in `postprocess_partitions`, leave the edges on the removed ports.
#[test]
fn a_removed_port_holds_no_edge() {
    let mut graph = graph(vec![node("a", Some(0)), node("b", Some(1))], vec![]);
    midprocess_partitions(&mut graph);

    postprocess_partitions(&mut graph).unwrap();

    for node in graph.layerless_nodes.iter() {
        assert!(node.ports.is_empty());
        // The removed ports keep their slots, so they can still be read.
        assert_eq!(node.ports.slot_count(), 1);
        let port = node.ports.get(PortId::new(0)).unwrap();
        assert!(port.incoming_edges().is_empty());
        assert!(port.outgoing_edges().is_empty());
    }
}

// Mutation: in `postprocess_partitions`, unlist every port, not only the marked ones.
#[test]
fn the_postprocessor_keeps_the_real_ports_of_a_node() {
    let mut graph = graph(
        vec![node("a", Some(0)), node("b", Some(1))],
        vec![edge("real", "a", "b", 0)],
    );
    let a = graph.node_named("a");
    let real_port = graph.layerless_nodes[a].ports.id_at(0).unwrap();
    midprocess_partitions(&mut graph);

    postprocess_partitions(&mut graph).unwrap();

    assert_eq!(
        graph.layerless_nodes[a].ports.ids().collect::<Vec<_>>(),
        [real_port]
    );
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(ends(&graph, EdgeId::new(0)), pair("a", "b"));
    assert_eq!(
        graph.layerless_nodes[a].ports[real_port].outgoing_edges(),
        [EdgeId::new(0)]
    );
}

// Mutation: in `postprocess_partitions`, pop one edge too many from the end of the edge list.
#[test]
fn only_the_marked_edges_leave_the_edge_list() {
    let mut graph = graph(
        vec![node("a", Some(0)), node("b", Some(1))],
        vec![edge("e1", "a", "b", 0), edge("e2", "b", "b", 0)],
    );
    midprocess_partitions(&mut graph);
    assert_eq!(graph.edges.len(), 3);

    postprocess_partitions(&mut graph).unwrap();

    let ids: Vec<&str> = graph.edges.iter().map(|edge| edge.id.as_str()).collect();
    assert_eq!(ids, ["e1", "e2"]);
}

// Mutation: in `postprocess_partitions`, cut the edge list at the first marked edge without
// checking what follows it.
#[test]
fn an_edge_added_after_the_partition_edges_is_an_error() {
    let mut graph = graph(vec![node("a", Some(0)), node("b", Some(1))], vec![]);
    midprocess_partitions(&mut graph);
    // A later step added an edge after the marked one.
    let a = graph.node_named("a");
    let b = graph.node_named("b");
    let source = graph
        .add_port(a, PortType::Output, PortSide::East, Default::default())
        .unwrap();
    let target = graph
        .add_port(b, PortType::Input, PortSide::West, Default::default())
        .unwrap();
    graph.add_edge(LayeredEdge::new("later", source, target));
    let before = graph.clone();

    let error = postprocess_partitions(&mut graph).unwrap_err();

    assert_eq!(
        error,
        IntermediateError::EdgeAfterPartitionEdges {
            edge_id: "later".to_string()
        }
    );
    // A failure changes nothing.
    assert_eq!(graph.edges, before.edges);
}
