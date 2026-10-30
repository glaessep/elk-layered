//! Tests of the classes in this module.

use crate::graph::LGraph;
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::{ElkDirection, LayeredOptions};
use crate::phase::cycles::break_cycles_greedy;
use crate::phase::layering::network_simplex_layerer::{iteration_limit, layer_network_simplex};

// Mutation: multiply in `usize` instead of wrapping in `i32`.
#[test]
fn the_iteration_limit_wraps_like_javas_int() {
    // 7 * 4 * (int) sqrt(10) = 84.
    assert_eq!(iteration_limit(7, 10), 84);
    // 2^29 * 4 wraps to i32::MIN, and the limit is then negative: no iterations.
    assert_eq!(iteration_limit(1 << 29, 10), 0);
    // (2^30 + 5) * 4 wraps to 20.
    assert_eq!(iteration_limit((1 << 30) + 5, 1), 20);
}

pub(super) fn node(id: &str) -> ElkInputNode {
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

pub(super) fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
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

pub(super) fn graph(
    nodes: Vec<ElkInputNode>,
    edges: Vec<ElkInputEdge>,
) -> crate::graph::GraphArena {
    import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap()
}

#[test]
fn network_simplex_layerer_assigns_forward_layers() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    );

    layer_network_simplex(&mut graph).unwrap();

    assert_eq!(graph.layers().len(), 3);
    assert_layer_order(&graph, "A", "B");
    assert_layer_order(&graph, "B", "C");
}

#[test]
fn network_simplex_layerer_ignores_self_loops() {
    let mut graph = graph(
        vec![node("A"), node("B")],
        vec![edge("A-A", "A", "A"), edge("A-B", "A", "B")],
    );

    layer_network_simplex(&mut graph).unwrap();

    assert_layer_order(&graph, "A", "B");
}

#[test]
fn network_simplex_layerer_processes_connected_components() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C"), node("D")],
        vec![edge("A-B", "A", "B"), edge("C-D", "C", "D")],
    );

    layer_network_simplex(&mut graph).unwrap();

    assert_layer_order(&graph, "A", "B");
    assert_layer_order(&graph, "C", "D");
    assert_eq!(graph.layers()[0].nodes().len(), 2);
}

#[test]
fn greedy_cycle_breaker_output_can_be_layered_by_network_simplex() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-A", "C", "A"),
        ],
    );

    break_cycles_greedy(&mut graph);
    layer_network_simplex(&mut graph).unwrap();

    for edge in &graph.edges {
        if edge.source().node == edge.target().node {
            continue;
        }
        let source_layer = graph.layerless_nodes[edge.source().node]
            .layer_index()
            .unwrap();
        let target_layer = graph.layerless_nodes[edge.target().node]
            .layer_index()
            .unwrap();
        assert!(
            target_layer > source_layer,
            "edge {} should point from an earlier layer to a later layer",
            edge.id
        );
    }
}

fn assert_layer_order(graph: &LGraph, source: &str, target: &str) {
    let source = graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == source)
        .unwrap();
    let target = graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == target)
        .unwrap();
    assert!(target.layer_index().unwrap() > source.layer_index().unwrap());
}
