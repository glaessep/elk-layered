//! Tests of the classes in this module.

use crate::graph::{EdgeId, IdVec, LGraph, NodeId};
use crate::graph::{EdgeLabelPlacement, LPoint, PortSide, PortType};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, import_graph};
use crate::intermediate::restore_reversed_edges;
use crate::options::LayerConstraint;
use crate::options::{ElkDirection, LayeredOptions};
use crate::phase::cycles::depth_first_cycle_breaker::break_cycles_depth_first;
use crate::phase::cycles::greedy_cycle_breaker::break_cycles_greedy;
use crate::phase::cycles::greedy_model_order_cycle_breaker::break_cycles_greedy_model_order;
use crate::phase::cycles::interactive_cycle_breaker::break_cycles_interactive;
use crate::phase::cycles::model_order_cycle_breaker::break_cycles_model_order;

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
    import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap()
}

#[test]
fn greedy_cycle_breaker_keeps_dag_edges_forward() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    );

    break_cycles_greedy(&mut graph);

    assert!(!graph.cyclic);
    assert!(graph.edges.iter().all(|edge| !edge.reversed));
    assert_eq!(graph.edges[EdgeId::new(0)].source_node_id, "A");
    assert_eq!(graph.edges[EdgeId::new(0)].target_node_id, "B");
}

#[test]
fn greedy_cycle_breaker_reverses_feedback_edges() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-A", "C", "A"),
        ],
    );

    break_cycles_greedy(&mut graph);

    assert!(graph.cyclic);
    assert!(graph.edges.iter().any(|edge| edge.reversed));
    assert!(!has_directed_cycle(&graph));
}

#[test]
fn greedy_model_order_cycle_breaker_chooses_lowest_model_order_tie() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-A", "C", "A"),
        ],
    );
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    let c = graph.node_named("C");
    graph.layerless_nodes[a].model_order = Some(3);
    graph.layerless_nodes[b].model_order = Some(1);
    graph.layerless_nodes[c].model_order = Some(2);

    break_cycles_greedy_model_order(&mut graph);

    assert!(graph.cyclic);
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.id == "A-B" && edge.reversed),
        "choosing B first makes the incoming A-B edge the feedback edge"
    );
    assert!(!has_directed_cycle(&graph));
}

#[test]
fn depth_first_cycle_breaker_reverses_back_edges() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-A", "C", "A"),
        ],
    );

    break_cycles_depth_first(&mut graph);

    assert!(graph.cyclic);
    assert!(graph.edges.iter().any(|edge| edge.reversed));
    assert!(!has_directed_cycle(&graph));
}

#[test]
fn interactive_cycle_breaker_reverses_edges_against_reference_x() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("B-A", "B", "A")]);
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    graph.layerless_nodes[a].position.x = 0.0;
    graph.layerless_nodes[b].position.x = 100.0;

    break_cycles_interactive(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].reversed);
    assert!(!has_directed_cycle(&graph));
}

#[test]
fn model_order_cycle_breaker_respects_first_layer_constraint() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("B-A", "B", "A")]);
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    graph.layerless_nodes[a].model_order = Some(1);
    graph.layerless_nodes[a].set_layer_constraint(crate::options::LayerConstraint::First);
    graph.layerless_nodes[b].model_order = Some(0);

    break_cycles_model_order(&mut graph);

    assert!(graph.cyclic);
    assert!(graph.edges[EdgeId::new(0)].reversed);
    assert!(!has_directed_cycle(&graph));
}

#[test]
fn model_order_cycle_breaker_consumes_separate_counters_in_edge_order() {
    let mut graph = graph(
        vec![
            node("first-source"),
            node("first-target"),
            node("last-source"),
            node("last-target"),
        ],
        vec![
            edge("first-0", "first-source", "first-target"),
            edge("first-1", "first-source", "first-target"),
            edge("last-0", "last-source", "last-target"),
            edge("last-1", "last-source", "last-target"),
        ],
    );
    for (id, constraint, model_order) in [
        ("first-source", LayerConstraint::FirstSeparate, Some(2)),
        ("first-target", LayerConstraint::FirstSeparate, Some(0)),
        ("last-source", LayerConstraint::LastSeparate, Some(2)),
        ("last-target", LayerConstraint::LastSeparate, Some(0)),
    ] {
        let node = graph
            .layerless_nodes
            .iter_mut()
            .find(|node| node.id == id)
            .unwrap();
        node.set_layer_constraint(constraint);
        node.model_order = model_order;
    }

    break_cycles_model_order(&mut graph);

    let reversed = graph
        .edges
        .iter()
        .filter(|edge| edge.reversed)
        .map(|edge| edge.id.as_str())
        .collect::<Vec<_>>();
    // elkjs 0.9.3 increments the separate counter for every target evaluation. With a
    // two-point model-order gap, only the first edge in each ordered pair compares lower.
    assert_eq!(reversed, ["first-0", "last-0"]);
}

#[test]
fn greedy_cycle_breaker_ignores_self_loops() {
    let mut graph = graph(vec![node("A")], vec![edge("A-A", "A", "A")]);

    break_cycles_greedy(&mut graph);

    assert!(!graph.cyclic);
    assert!(!graph.edges[EdgeId::new(0)].reversed);
}

// Mutation: in `restore_reversed_edges`, skip the edges that are marked reversed.
#[test]
fn reversed_edge_restorer_restores_original_direction() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-A", "C", "A"),
        ],
    );
    let original_endpoints = graph
        .edges
        .iter()
        .map(|edge| (edge.source(), edge.target()))
        .collect::<Vec<_>>();

    break_cycles_greedy(&mut graph);
    let all_nodes = graph.node_ids().collect();
    graph.set_layer_nodes(0, all_nodes);
    restore_reversed_edges(&mut graph);

    assert!(graph.edges.iter().all(|edge| !edge.reversed));
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|edge| (edge.source(), edge.target()))
            .collect::<Vec<_>>(),
        original_endpoints
    );
}

#[test]
fn reverse_edge_swaps_endpoint_adjacency_labels_and_bendpoints() {
    let mut ab = edge("A-B", "A", "B");
    ab.labels = vec![ElkInputLabel {
        source_index: None,
        text: "end".to_string(),
        width: 20.0,
        height: 12.0,
        placement: EdgeLabelPlacement::Head,
        inline: false,
        node_label_placement: None,
        x: 0.0,
        y: 0.0,
    }];
    let mut graph = graph(vec![node("A"), node("B")], vec![ab]);
    graph.edges[EdgeId::new(0)].bend_points =
        vec![LPoint { x: 1.0, y: 2.0 }, LPoint { x: 3.0, y: 4.0 }];
    let old_source = graph.edges[EdgeId::new(0)].source();
    let old_target = graph.edges[EdgeId::new(0)].target();

    assert!(crate::graph::reverse_edge(&mut graph, EdgeId::new(0), true));

    assert_eq!(graph.edges[EdgeId::new(0)].source(), old_target);
    assert_eq!(graph.edges[EdgeId::new(0)].target(), old_source);
    assert!(
        graph.layerless_nodes[old_source.node].ports[old_source.port]
            .outgoing_edges()
            .is_empty()
    );
    assert_eq!(
        graph.layerless_nodes[old_source.node].ports[old_source.port].incoming_edges(),
        vec![EdgeId::new(0)]
    );
    assert_eq!(
        graph.layerless_nodes[old_target.node].ports[old_target.port].outgoing_edges(),
        vec![EdgeId::new(0)]
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)].labels[0].placement,
        EdgeLabelPlacement::Tail
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 3.0, y: 4.0 }, LPoint { x: 1.0, y: 2.0 }]
    );
    assert!(graph.edges[EdgeId::new(0)].reversed);
}

#[test]
fn reverse_edge_adapts_collector_ports_like_elk() {
    let mut input = ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A"), node("B")],
        edges: vec![edge("A-B", "A", "B")],
    };
    input.options.merge_edges = true;
    let mut graph = import_graph(&input).unwrap();
    let old_source = graph.edges[EdgeId::new(0)].source();
    let old_target = graph.edges[EdgeId::new(0)].target();

    assert_eq!(
        graph.layerless_nodes[old_source.node].ports[old_source.port].collector_type,
        Some(PortType::Output)
    );
    assert_eq!(
        graph.layerless_nodes[old_target.node].ports[old_target.port].collector_type,
        Some(PortType::Input)
    );

    assert!(crate::graph::reverse_edge(&mut graph, EdgeId::new(0), true));

    let new_source = graph.edges[EdgeId::new(0)].source();
    let new_target = graph.edges[EdgeId::new(0)].target();
    assert_eq!(new_source.node, old_target.node);
    assert_eq!(new_target.node, old_source.node);
    assert_ne!(new_source.port, old_target.port);
    assert_ne!(new_target.port, old_source.port);
    assert_eq!(
        graph.layerless_nodes[new_source.node].ports[new_source.port].collector_type,
        Some(PortType::Output)
    );
    assert_eq!(
        graph.layerless_nodes[new_source.node].ports[new_source.port].side,
        PortSide::East
    );
    assert_eq!(
        graph.layerless_nodes[new_target.node].ports[new_target.port].collector_type,
        Some(PortType::Input)
    );
    assert_eq!(
        graph.layerless_nodes[new_target.node].ports[new_target.port].side,
        PortSide::West
    );
    assert!(
        graph.layerless_nodes[old_source.node].ports[old_source.port]
            .outgoing_edges()
            .is_empty()
    );
    assert!(
        graph.layerless_nodes[old_target.node].ports[old_target.port]
            .incoming_edges()
            .is_empty()
    );
    assert_eq!(
        graph.layerless_nodes[new_source.node].ports[new_source.port].outgoing_edges(),
        vec![EdgeId::new(0)]
    );
    assert_eq!(
        graph.layerless_nodes[new_target.node].ports[new_target.port].incoming_edges(),
        vec![EdgeId::new(0)]
    );
}

fn has_directed_cycle(graph: &LGraph) -> bool {
    let mut state = IdVec::filled(graph.layerless_nodes.len(), VisitState::Unseen);
    graph.node_ids().any(|node| visit(graph, node, &mut state))
}

fn visit(graph: &LGraph, node: NodeId, state: &mut IdVec<NodeId, VisitState>) -> bool {
    match state[node] {
        VisitState::Active => return true,
        VisitState::Done => return false,
        VisitState::Unseen => {}
    }

    state[node] = VisitState::Active;
    for port in &graph.layerless_nodes[node].ports {
        for edge_index in port.outgoing_edges() {
            let edge = &graph.edges[*edge_index];
            if edge.source().node == edge.target().node {
                continue;
            }
            if visit(graph, edge.target().node, state) {
                return true;
            }
        }
    }
    state[node] = VisitState::Done;
    false
}

#[derive(Clone, Copy)]
enum VisitState {
    Unseen,
    Active,
    Done,
}

// Mutation: let `collect_interactive_back_edges` recurse again.
#[test]
fn a_cycle_through_10000_nodes_breaks_interactively_on_a_small_stack() {
    const DEPTH: usize = 10_000;
    let nodes = (0..DEPTH).map(|i| node(&format!("n{i}"))).collect();
    let mut edges: Vec<_> = (1..DEPTH)
        .map(|i| edge(&format!("e{i}"), &format!("n{}", i - 1), &format!("n{i}")))
        .collect();
    edges.push(edge("back", &format!("n{}", DEPTH - 1), "n0"));
    let mut graph = graph(nodes, edges);
    // Every node sits at the same x, so the first pass reverses nothing and the depth-first pass
    // walks the whole chain to find the back edge.
    let reversed = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            break_cycles_interactive(&mut graph);
            graph.edges.iter().filter(|edge| edge.reversed).count()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(reversed, 1);
}
