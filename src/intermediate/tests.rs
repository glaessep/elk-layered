//! Tests of the classes in this module.

use std::collections::HashMap;

use crate::graph::{
    EdgeId, EdgeLabelPlacement, GraphId, LGraph, LNode, LNodeKind, LPoint, LabelSide, LayeredEdge,
    NodeId, PortId, PortRef, PortSide,
};
use crate::graph::{InLayerConstraint, LLabel, LSize, PortType};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, import_graph};
use crate::intermediate::IntermediateError;
use crate::intermediate::edge_and_layer_constraint_edge_reverser::reverse_edges_for_edge_and_layer_constraints;
use crate::intermediate::end_label_preprocessor::{
    copy_gathered_end_labels_into_ports, gather_end_labels_for_node, preprocess_end_labels,
    remove_gathered_end_labels,
};
use crate::intermediate::hierarchical_port_constraint_processor::process_hierarchical_port_constraints;
use crate::intermediate::hierarchical_port_dummy_size_processor::process_hierarchical_port_dummy_sizes;
use crate::intermediate::hierarchical_port_orthogonal_edge_router::{
    connect_replacement_dummy_to_original, process_hierarchical_port_orthogonal_edges,
    remove_temporary_north_south_dummies, restore_north_south_dummies,
};
use crate::intermediate::hierarchical_port_position_processor::process_hierarchical_port_positions;
use crate::intermediate::hyperedge_dummy_merger::merge_hyperedge_dummies;
use crate::intermediate::inverted_port_processor::process_inverted_ports;
use crate::intermediate::label_dummy_inserter::{create_label_dummy_node, insert_label_dummies};
use crate::intermediate::label_dummy_remover::{place_label_dummy_labels, remove_label_dummies};
use crate::intermediate::label_dummy_switcher::{
    LabelDummyInfo, find_median_layer_target_id, switch_label_dummies,
};
use crate::intermediate::label_side_selector::{apply_label_side, select_label_sides};
use crate::intermediate::layer_constraint_postprocessor::postprocess_layer_constraints;
use crate::intermediate::layer_constraint_preprocessor::preprocess_layer_constraints;
use crate::intermediate::layer_size_and_graph_height_calculator::calculate_layer_sizes_and_graph_height;
use crate::intermediate::long_edge_joiner::{join_long_edge_at_indexed, join_long_edges};
use crate::intermediate::long_edge_splitter::{split_edge, split_long_edges};
use crate::options::{Alignment, EdgeLabelSideSelection};
use crate::options::{ElkDirection, LayerConstraint, LayeredOptions, PortConstraints};
use crate::phase::layering::layer_network_simplex;

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
    import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap()
}

#[test]
fn edge_and_layer_constraint_reverser_makes_first_nodes_outgoing_only() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::First);
    let mut graph = graph(vec![node("A"), first], vec![edge("A-start", "A", "start")]);
    let start = graph.node_named("start");

    reverse_edges_for_edge_and_layer_constraints(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].reversed);
    assert_eq!(graph.edges[EdgeId::new(0)].source().node, start);
    assert!(graph.node_incoming_edges(start).is_empty());
    assert_eq!(graph.node_outgoing_edges(start), vec![EdgeId::new(0)]);
}

#[test]
fn edge_and_layer_constraint_reverser_makes_last_nodes_incoming_only() {
    let mut last = node("end");
    last.layer_constraint = Some(LayerConstraint::Last);
    let mut graph = graph(vec![last, node("A")], vec![edge("end-A", "end", "A")]);
    let end = graph.node_named("end");

    reverse_edges_for_edge_and_layer_constraints(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].reversed);
    assert_eq!(graph.edges[EdgeId::new(0)].target().node, end);
    assert_eq!(graph.node_incoming_edges(end), vec![EdgeId::new(0)]);
    assert!(graph.node_outgoing_edges(end).is_empty());
}

#[test]
fn constraint_reversal_candidates_follow_fixed_side_cascades() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("B-A", "B", "A"), edge("C-B", "C", "B")],
    );
    for node_id in ["A", "B"] {
        let node_index = graph.node_named(node_id);
        graph.layerless_nodes[node_index].port_constraints = PortConstraints::FixedSide;
        for port in graph.layerless_nodes[node_index].ports.slots_mut() {
            port.set_side(PortSide::East);
        }
    }

    reverse_edges_for_edge_and_layer_constraints(&mut graph);

    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.reversed)
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["B-A", "C-B"]
    );
}

#[test]
fn direct_reversal_skips_ineligible_fixed_collector_edges() {
    let parallel_edge_count = 128usize;
    let mut edges = (0..parallel_edge_count)
        .map(|index| edge(&format!("F-U-{index}"), "F", "U"))
        .collect::<Vec<_>>();
    edges.push(edge("F-D", "F", "D"));
    let mut first = node("D");
    first.layer_constraint = Some(LayerConstraint::First);
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            merge_edges: true,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        },
        nodes: vec![node("F"), node("U"), first],
        edges,
    })
    .unwrap();
    let fixed = graph.node_named("F");
    graph.layerless_nodes[fixed].port_constraints = PortConstraints::FixedSide;
    for port in graph.layerless_nodes[fixed].ports.slots_mut() {
        port.set_side(PortSide::East);
    }

    reverse_edges_for_edge_and_layer_constraints(&mut graph);
    assert_eq!(graph.edges.iter().filter(|edge| edge.reversed).count(), 1);
    assert!(
        graph
            .edges
            .iter()
            .find(|edge| edge.id == "F-D")
            .unwrap()
            .reversed
    );
}

#[test]
fn long_edge_splitter_makes_layering_proper() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C"), node("D")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-D", "C", "D"),
            edge("A-D", "A", "D"),
        ],
    );

    layer_network_simplex(&mut graph).unwrap();
    split_long_edges(&mut graph);

    assert!(
        graph
            .layerless_nodes
            .iter()
            .any(|node| node.kind() == LNodeKind::LongEdge)
    );
    for edge in &graph.edges {
        let source_layer = graph.layerless_nodes[edge.source().node]
            .layer_index()
            .unwrap();
        let target_layer = graph.layerless_nodes[edge.target().node]
            .layer_index()
            .unwrap();
        assert_eq!(
            target_layer,
            source_layer + 1,
            "edge {} should connect adjacent layers",
            edge.id
        );
    }
}

#[test]
fn hyperedge_dummy_merger_coalesces_parallel_long_edge_segments() {
    let mut input_graph = ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A"), node("B"), node("C"), node("D"), node("E")],
        edges: vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("C-D", "C", "D"),
            edge("D-E", "D", "E"),
            edge("A-E-1", "A", "E"),
            edge("A-E-2", "A", "E"),
        ],
    };
    input_graph.options.merge_edges = true;
    let mut input = import_graph(&input_graph).unwrap();

    layer_network_simplex(&mut input).unwrap();
    split_long_edges(&mut input);

    let before = input
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .filter(|node| input.layerless_nodes[*node].kind() == LNodeKind::LongEdge)
        .count();
    assert!(before >= 4);

    merge_hyperedge_dummies(&mut input);

    let after = input
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .filter(|node| input.layerless_nodes[*node].kind() == LNodeKind::LongEdge)
        .count();
    assert!(after < before);
    assert!(
        input
            .edges_with_ids()
            .all(|(edge_index, _)| input.edge_source_attached(edge_index))
    );
    assert!(
        input
            .edges_with_ids()
            .all(|(edge_index, _)| input.edge_target_attached(edge_index))
    );
}

#[test]
fn split_edge_moves_head_labels_to_new_segment_and_records_origin_ports() {
    let mut head = ElkInputLabel::center("head", 20.0, 10.0);
    head.placement = EdgeLabelPlacement::Head;
    let mut long = edge("A-C", "A", "C");
    long.labels = vec![head];

    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C"), long],
    );

    layer_network_simplex(&mut graph).unwrap();
    let long_edge = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-C")
            .unwrap(),
    );
    let original_source = graph.edges[long_edge].source();
    let original_target = graph.edges[long_edge].target();

    split_long_edges(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::LongEdge)
            .unwrap(),
    );
    assert_eq!(graph.layerless_nodes[dummy].origin_edge(), Some(long_edge));
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .source,
        Some(original_source)
    );
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .target,
        Some(original_target)
    );

    assert!(graph.edges[long_edge].labels.is_empty());
    let moved_label_edge = graph
        .edges_with_ids()
        .find(|(_, edge)| edge.labels.iter().any(|label| label.text == "head"))
        .map(|(index, _)| index)
        .unwrap();
    assert_ne!(moved_label_edge, long_edge);
    assert_eq!(
        graph.edges[moved_label_edge].labels[0].end_label_edge,
        Some(long_edge)
    );
}

#[test]
fn inverted_port_processor_splits_east_input_ports() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let source = push_normal_node(&mut graph, "A");
    let target = push_normal_node(&mut graph, "B");
    graph.layerless_nodes[target].port_constraints = PortConstraints::FixedSide;
    graph.layerless_nodes[target].ports[PortId::new(0)].set_side(PortSide::East);
    graph.set_node_layer(source, 0);
    graph.set_node_layer(target, 1);
    let edge = add_test_edge(&mut graph, "A-B", source, 0, target, 0);
    let mut head = LLabel::new("head", 10.0, 8.0);
    head.placement = EdgeLabelPlacement::Head;
    graph.edges[edge].labels.push(head);

    process_inverted_ports(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::LongEdge)
            .expect("inverted input should create a long-edge dummy"),
    );
    assert_eq!(graph.layerless_nodes[dummy].layer_index(), Some(1));
    assert_eq!(graph.edges[edge].target().node, dummy);
    assert_eq!(graph.edges[edge].target().port, PortId::new(0));
    assert!(graph.edges[edge].labels.is_empty());

    let dummy_edge = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|candidate| {
                candidate.source().node == dummy && candidate.target().node == target
            })
            .expect("dummy should reconnect to original target port"),
    );
    assert_eq!(graph.edges[dummy_edge].source().port, PortId::new(1));
    assert_eq!(graph.edges[dummy_edge].target().port, PortId::new(0));
    assert_eq!(graph.edges[dummy_edge].labels[0].text, "head");
    assert_eq!(graph.edges[dummy_edge].labels[0].end_label_edge, Some(edge));
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .source,
        Some(graph.edges[edge].source())
    );
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .target,
        Some(graph.edges[dummy_edge].target())
    );
}

#[test]
fn inverted_port_processor_splits_west_output_ports() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let source = push_normal_node(&mut graph, "A");
    let target = push_normal_node(&mut graph, "B");
    graph.layerless_nodes[source].port_constraints = PortConstraints::FixedSide;
    graph.layerless_nodes[source].ports[PortId::new(0)].port_type = PortType::Output;
    graph.layerless_nodes[source].ports[PortId::new(0)].set_side(PortSide::West);
    graph.set_node_layer(source, 0);
    graph.set_node_layer(target, 1);
    let edge = add_test_edge(&mut graph, "A-B", source, 0, target, 0);
    let mut head = LLabel::new("head", 10.0, 8.0);
    head.placement = EdgeLabelPlacement::Head;
    graph.edges[edge].labels.push(head);

    process_inverted_ports(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::LongEdge)
            .expect("inverted output should create a long-edge dummy"),
    );
    assert_eq!(graph.layerless_nodes[dummy].layer_index(), Some(0));
    assert_eq!(graph.edges[edge].source().node, source);
    assert_eq!(graph.edges[edge].target().node, dummy);
    assert_eq!(graph.edges[edge].target().port, PortId::new(0));
    assert!(graph.edges[edge].labels.is_empty());

    let dummy_edge = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|candidate| {
                candidate.source().node == dummy && candidate.target().node == target
            })
            .expect("dummy should reconnect to original target port"),
    );
    assert_eq!(graph.edges[dummy_edge].source().port, PortId::new(1));
    assert_eq!(graph.edges[dummy_edge].labels[0].text, "head");
    assert_eq!(graph.edges[dummy_edge].labels[0].end_label_edge, Some(edge));
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .source,
        Some(graph.edges[edge].source())
    );
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .target,
        Some(graph.edges[dummy_edge].target())
    );
}

#[test]
fn label_dummy_inserter_splits_center_label_edges_and_moves_labels_to_dummy() {
    let mut center = ElkInputLabel::center("choice", 30.0, 12.0);
    center.placement = EdgeLabelPlacement::Center;
    let mut labelled = edge("A-B", "A", "B");
    labelled.labels = vec![center];
    let mut graph = graph(vec![node("A"), node("B")], vec![labelled]);
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-B")
            .unwrap(),
    );
    let original_source = graph.edges[edge_index].source();
    let original_target = graph.edges[edge_index].target();

    insert_label_dummies(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::Label)
            .unwrap(),
    );
    assert_eq!(graph.layerless_nodes[dummy].origin_edge(), Some(edge_index));
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .source,
        Some(original_source)
    );
    assert_eq!(
        graph.layerless_nodes[dummy]
            .long_edge_ends()
            .unwrap()
            .target,
        Some(original_target)
    );
    assert!(graph.layerless_nodes[dummy].labels.is_empty());
    assert_eq!(graph.layerless_nodes[dummy].represented_labels().len(), 1);
    assert_eq!(
        graph.layerless_nodes[dummy].represented_labels()[0].text,
        "choice"
    );
    assert!(graph.edges[edge_index].labels.is_empty());
    assert_eq!(graph.node_incoming_edges(dummy).len(), 1);
    assert_eq!(graph.node_outgoing_edges(dummy).len(), 1);
}

// Mutation: in `size_label_dummy`, start the height at 0 instead of at the edge thickness.
#[test]
fn horizontal_inline_label_dummy_retains_split_edge_thickness() {
    let (mut graph, dummy, _) = label_dummy_graph(
        ElkDirection::Right,
        1.0,
        vec![ElkInputLabel::center("choice", 30.0, 12.0)],
    );
    // The thickness 1, the label 12, the spacing 7 and the thickness 1 again.
    assert_eq!(graph.layerless_nodes[dummy].size.height, 21.0);

    apply_label_side(&mut graph, dummy, LabelSide::Below);

    assert_eq!(graph.layerless_nodes[dummy].size.height, 13.0);
    assert_dummy_ports_at(&graph, dummy, 6.5);
}

/// A graph of one edge A-B with centre labels, a label dummy for it, and the numbers the
/// tests below need: edge label spacing 7 and the given edge thickness. The option that sets
/// a thickness is not ported, so only a test can give an edge another thickness than 1.
fn label_dummy_graph(
    direction: ElkDirection,
    thickness: f64,
    labels: Vec<ElkInputLabel>,
) -> (crate::graph::GraphArena, NodeId, EdgeId) {
    let mut labelled = edge("A-B", "A", "B");
    labelled.labels = labels;
    let mut graph = graph(vec![node("A"), node("B")], vec![labelled]);
    graph.options.direction = direction;
    graph.options.spacing.edge_label = 7.0;
    graph.edges[EdgeId::new(0)].thickness = thickness;

    insert_label_dummies(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::Label)
            .unwrap(),
    );
    let edge_index = graph.layerless_nodes[dummy].origin_edge().unwrap();
    (graph, dummy, edge_index)
}

/// Asserts that the dummy has exactly its input and output port, both at height `y`. A bare
/// `.all(..)` over the ports passes when the dummy has none.
fn assert_dummy_ports_at(graph: &LGraph, dummy: NodeId, y: f64) {
    let ports = &graph.layerless_nodes[dummy].ports;
    assert_eq!(ports.len(), 2);
    for port in ports {
        assert_eq!(port.position.y, y);
    }
}

// Mutation: in the inline arm of `apply_label_side`, subtract `(origin_edge / 2.0).ceil()`
// instead of `origin_edge`, from the height or from the port position.
#[test]
fn inline_label_dummy_gives_up_the_edge_thickness_and_the_label_spacing() {
    // The dummy is 12.5 + 7 + 2 * 5 = 29.5 high. Inline, it gives up 7 and 5.
    let (mut graph, dummy, _) = label_dummy_graph(
        ElkDirection::Right,
        5.0,
        vec![ElkInputLabel::center("choice", 30.0, 12.5)],
    );
    assert_eq!(graph.layerless_nodes[dummy].size.height, 29.5);

    apply_label_side(&mut graph, dummy, LabelSide::Below);

    assert_eq!(
        graph.layerless_nodes[dummy].label_dummy().unwrap().side,
        LabelSide::Inline
    );
    assert_eq!(graph.layerless_nodes[dummy].size.height, 17.5);
    // ceil(17.5) / 2, not 17.5 / 2.
    assert_dummy_ports_at(&graph, dummy, 9.0);
}

// Mutation: in `apply_label_side`, leave out the `.max(0.0)` of the edge thickness.
#[test]
fn a_negative_edge_thickness_counts_as_zero_in_apply_label_side() {
    for (side, height, port_y) in [
        // The inline arm: nothing but the spacing 7 comes off 19.5, and ceil(12.5) / 2.
        (LabelSide::Below, 12.5, 6.5),
        // The above arm: the port sits at the bottom of the dummy.
        (LabelSide::Above, 19.5, 19.5),
    ] {
        let mut center = ElkInputLabel::center("choice", 30.0, 12.5);
        center.inline = side == LabelSide::Below;
        let (mut graph, dummy, edge_index) =
            label_dummy_graph(ElkDirection::Right, 0.0, vec![center]);
        // A thickness the importer never gives, set after the dummy was sized.
        graph.edges[edge_index].thickness = -3.0;

        apply_label_side(&mut graph, dummy, side);

        assert_eq!(graph.layerless_nodes[dummy].size.height, height, "{side:?}");
        assert_dummy_ports_at(&graph, dummy, port_y);
    }
}

// Mutation: in `insert_label_dummies`, leave out the `.max(0.0)` of the edge thickness.
#[test]
fn a_negative_edge_thickness_counts_as_zero_when_the_label_dummy_is_made() {
    // A thickness the importer never gives, set before the dummy is made.
    let (graph, dummy, _) = label_dummy_graph(
        ElkDirection::Right,
        -3.0,
        vec![ElkInputLabel::center("choice", 30.0, 12.5)],
    );

    // Both halves of the split edge keep thickness 0.
    assert_eq!(graph.edges.len(), 2);
    assert!(graph.edges.iter().all(|edge| edge.thickness == 0.0));
    // The label 12.5 and the spacing 7, with nothing for the edge.
    assert_eq!(graph.layerless_nodes[dummy].size.height, 19.5);
    assert_dummy_ports_at(&graph, dummy, 0.0);
}

// Mutation: in `split_edge`, leave out the `.max(0.0)` of the edge thickness.
#[test]
fn split_edge_counts_a_negative_edge_thickness_as_zero() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
    graph.edges[EdgeId::new(0)].thickness = -3.0;
    let dummy = create_label_dummy_node(&mut graph, EdgeId::new(0), 0.0);

    let second_half = split_edge(&mut graph, EdgeId::new(0), dummy).unwrap();

    assert_eq!(graph.edges[EdgeId::new(0)].thickness, 0.0);
    assert_eq!(graph.edges[second_half].thickness, 0.0);
    assert_eq!(graph.layerless_nodes[dummy].size.height, 0.0);
    // floor(0 / 2), not floor(-3 / 2).
    assert_dummy_ports_at(&graph, dummy, 0.0);
}

// Mutation: in `place_label_dummy_labels`, leave the edge thickness out of the shift of a
// label below the edge.
#[test]
fn labels_below_an_edge_start_one_thickness_and_one_spacing_below_the_dummy() {
    let mut plain = ElkInputLabel::center("choice", 30.0, 12.5);
    plain.inline = false;
    let (mut graph, dummy, edge_index) = label_dummy_graph(ElkDirection::Right, 5.0, vec![plain]);
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };
    graph.layerless_nodes[dummy].label_dummy_mut().unwrap().side = LabelSide::Below;

    place_label_dummy_labels(&mut graph, dummy);

    // 40 + thickness 5 + spacing 7. The label is 30 wide in a dummy 30 wide.
    let label = &graph.edges[edge_index].labels[0];
    assert_eq!(label.position, LPoint { x: 20.0, y: 52.0 });
}

// Mutation: in `place_label_dummy_labels`, leave out the `.max(0.0)` of the edge thickness.
#[test]
fn a_negative_edge_thickness_counts_as_zero_in_place_label_dummy_labels() {
    let mut plain = ElkInputLabel::center("choice", 30.0, 12.5);
    plain.inline = false;
    let (mut graph, dummy, edge_index) = label_dummy_graph(ElkDirection::Right, 0.0, vec![plain]);
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };
    graph.layerless_nodes[dummy].label_dummy_mut().unwrap().side = LabelSide::Below;
    graph.edges[edge_index].thickness = -3.0;

    place_label_dummy_labels(&mut graph, dummy);

    // 40 + 0 + spacing 7, not 40 - 3 + 7.
    assert_eq!(graph.edges[edge_index].labels[0].position.y, 47.0);
}

// Mutation: in `place_label_dummy_labels`, take the space for the labels as if they were not
// inline (`if false`), or leave the edge thickness out of the space of labels that are not.
#[test]
fn vertical_labels_sit_in_the_space_the_dummy_keeps_for_them() {
    let centre_labels = |inline: bool| {
        let mut tall = ElkInputLabel::center("choice", 30.0, 8.0);
        let mut short = ElkInputLabel::center("on", 12.0, 4.0);
        tall.inline = inline;
        short.inline = inline;
        vec![tall, short]
    };

    // Inline: the dummy is 8 + 7 + 5 = 20 high and gives up 7 + 5, so 8 are left. Both
    // labels are centred in them, side by side with the label spacing between.
    let (mut graph, dummy, edge_index) =
        label_dummy_graph(ElkDirection::Down, 5.0, centre_labels(true));
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };
    apply_label_side(&mut graph, dummy, LabelSide::Below);
    place_label_dummy_labels(&mut graph, dummy);
    let labels = &graph.edges[edge_index].labels;
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].position.y, 40.0);
    assert_eq!(labels[1].position.y, 42.0);

    // Not inline, above the edge: the labels end at the bottom of the 20 - 5 - 7 = 8 that
    // the dummy keeps for them.
    let (mut graph, dummy, edge_index) =
        label_dummy_graph(ElkDirection::Down, 5.0, centre_labels(false));
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };
    graph.layerless_nodes[dummy].label_dummy_mut().unwrap().side = LabelSide::Above;
    place_label_dummy_labels(&mut graph, dummy);
    let labels = &graph.edges[edge_index].labels;
    assert_eq!(labels[0].position.y, 40.0);
    assert_eq!(labels[1].position.y, 44.0);
}

#[test]
fn default_median_label_dummy_strategy_uses_lower_median_layer() {
    let info = |leftmost_layer_id, rightmost_layer_id| LabelDummyInfo {
        label_dummy: NodeId::new(0),
        left_long_edge_dummies: Vec::new(),
        right_long_edge_dummies: Vec::new(),
        leftmost_layer_id,
        rightmost_layer_id,
    };

    assert_eq!(find_median_layer_target_id(&info(3, 4)), 3);
    assert_eq!(find_median_layer_target_id(&info(3, 5)), 4);
}

#[test]
fn label_dummy_remover_restores_center_labels_to_origin_edge() {
    let mut center = ElkInputLabel::center("choice", 30.0, 12.0);
    center.placement = EdgeLabelPlacement::Center;
    let mut labelled = edge("A-C", "A", "C");
    labelled.labels = vec![center];
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C"), labelled],
    );
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-C")
            .unwrap(),
    );

    insert_label_dummies(&mut graph);
    layer_network_simplex(&mut graph).unwrap();
    split_long_edges(&mut graph);
    switch_label_dummies(&mut graph);
    select_label_sides(&mut graph);
    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::Label)
            .unwrap(),
    );
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };

    remove_label_dummies(&mut graph);

    assert!(!graph.layers().iter().any(|layer| {
        layer
            .nodes()
            .iter()
            .any(|node| graph.layerless_nodes[*node].kind() == LNodeKind::Label)
    }));
    assert!(graph.layerless_nodes[dummy].represented_labels().is_empty());
    assert_eq!(graph.edges[edge_index].labels.len(), 1);
    assert_eq!(graph.edges[edge_index].labels[0].text, "choice");
    assert!(
        !graph.edge_source_attached(
            graph
                .edges_with_ids()
                .find(|(index, edge)| *index != edge_index && edge.id == "A-C")
                .map(|(index, _)| index)
                .unwrap()
        )
    );
}

// Mutation: sort the gathered labels by ascending index in `remove_gathered_end_labels`. Each
// removal then shifts the next one, and the edge keeps head labels and loses center labels.
#[test]
fn end_label_gather_preserves_large_mixed_label_order_and_identity() {
    let mut input = edge("A-B", "A", "B");
    input.labels = (0..4096)
        .map(|index| {
            let mut label = ElkInputLabel::center(index.to_string(), 12.0, 10.0);
            label.placement = if index % 3 == 0 {
                EdgeLabelPlacement::Center
            } else {
                EdgeLabelPlacement::Head
            };
            label.source_index = Some(index);
            label
        })
        .collect();
    let mut graph = graph(vec![node("A"), node("B")], vec![input]);
    let target = graph.edges[EdgeId::new(0)].target();
    let gathered = gather_end_labels_for_node(&mut graph, target.node);
    copy_gathered_end_labels_into_ports(&mut graph, target.node, &gathered);
    remove_gathered_end_labels(&mut graph, gathered.labels.into_iter().flatten().collect());
    let port = &graph.layerless_nodes[target.node].ports[target.port];
    assert_eq!(
        port.labels
            .iter()
            .map(|label| label.source_index.unwrap())
            .collect::<Vec<_>>(),
        (0..4096).filter(|index| index % 3 != 0).collect::<Vec<_>>()
    );
    assert!(
        port.labels
            .iter()
            .all(|label| label.end_label_edge == Some(EdgeId::new(0)))
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)]
            .labels
            .iter()
            .map(|label| label.source_index.unwrap())
            .collect::<Vec<_>>(),
        (0..4096).filter(|index| index % 3 == 0).collect::<Vec<_>>()
    );
}

// Mutation: copy the gathered labels into the ports in `gather_end_labels_for_node` again. A
// dummy node then clones its labels for a margin and drops them.
#[test]
fn gathering_end_labels_copies_nothing_until_the_node_asks() {
    let mut head = ElkInputLabel::center("head", 24.0, 10.0);
    head.placement = EdgeLabelPlacement::Head;
    let mut labelled = edge("A-B", "A", "B");
    labelled.labels = vec![head];
    let mut graph = graph(vec![node("A"), node("B")], vec![labelled]);
    let target = graph.edges[EdgeId::new(0)].target();

    let gathered = gather_end_labels_for_node(&mut graph, target.node);

    assert_eq!(gathered.labels[target.port.index()].len(), 1);
    assert!(
        graph.layerless_nodes[target.node].ports[target.port]
            .labels
            .is_empty()
    );
    assert_eq!(graph.edges[EdgeId::new(0)].labels.len(), 1);
    copy_gathered_end_labels_into_ports(&mut graph, target.node, &gathered);
    assert_eq!(
        graph.layerless_nodes[target.node].ports[target.port]
            .labels
            .len(),
        1
    );
}

#[test]
fn end_label_preprocessor_moves_head_labels_to_port_cell_and_expands_margin() {
    let mut head = ElkInputLabel::center("head", 24.0, 10.0);
    head.placement = EdgeLabelPlacement::Head;
    head.inline = false;
    let mut labelled = edge("A-B", "A", "B");
    labelled.labels = vec![head];
    let mut graph = graph(vec![node("A"), node("B")], vec![labelled]);

    layer_network_simplex(&mut graph).unwrap();
    crate::intermediate::process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
    crate::intermediate::calculate_label_and_node_sizes(&mut graph);
    crate::intermediate::calculate_innermost_node_margins(&mut graph);
    select_label_sides(&mut graph);
    let target = graph.edges[EdgeId::new(0)].target().node;
    let target_port = graph.edges[EdgeId::new(0)].target().port;
    let old_left_margin = graph.layerless_nodes[target].margin.left;

    preprocess_end_labels(&mut graph);

    assert!(graph.edges[EdgeId::new(0)].labels.is_empty());
    let port = &graph.layerless_nodes[target].ports[target_port];
    assert_eq!(port.labels.len(), 1);
    assert_eq!(port.labels[0].text, "head");
    assert_eq!(port.labels[0].end_label_edge, Some(EdgeId::new(0)));
    assert!(port.end_label_cell.is_some());
    assert!(graph.layerless_nodes[target].margin.left > old_left_margin);
}

// Mutation: in `smart_for_regular_node`, queue a port only when it has an end label. The
// two ports of A then no longer pair up, and the tail label gets the default side.
#[test]
fn smart_label_sides_count_a_port_whose_edges_carry_no_end_label() {
    let mut tail = ElkInputLabel::center("tail", 24.0, 10.0);
    tail.placement = EdgeLabelPlacement::Tail;
    let mut labelled = edge("A-B", "A", "B");
    labelled.labels = vec![tail];
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![labelled, edge("A-C", "A", "C")],
    );
    graph.options.edge_label_side_selection = EdgeLabelSideSelection::SmartDown;

    layer_network_simplex(&mut graph).unwrap();
    crate::intermediate::process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
    select_label_sides(&mut graph);

    // A has one port per edge, and both sit on the same side. ELK pairs them up: the first
    // gets the label side that is not the default, here `Above`, whatever labels it has.
    let source = graph.edges[EdgeId::new(0)].source();
    let second = graph.edges[EdgeId::new(1)].source();
    assert_eq!(source.node, second.node);
    assert_eq!(
        graph.layerless_nodes[source.node].ports[source.port].side,
        graph.layerless_nodes[second.node].ports[second.port].side
    );
    assert!(
        graph.layerless_nodes[source.node]
            .ports
            .position(source.port)
            < graph.layerless_nodes[second.node]
                .ports
                .position(second.port)
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)].labels[0].label_side,
        Some(LabelSide::Above)
    );
}

#[test]
fn label_dummy_remover_centers_vertical_inline_labels_in_label_space() {
    let mut center = ElkInputLabel::center("choice", 30.0, 12.0);
    center.placement = EdgeLabelPlacement::Center;
    let mut labelled = edge("A-C", "A", "C");
    labelled.labels = vec![center];
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C"), labelled],
    );
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-C")
            .unwrap(),
    );

    insert_label_dummies(&mut graph);
    layer_network_simplex(&mut graph).unwrap();
    split_long_edges(&mut graph);
    switch_label_dummies(&mut graph);
    select_label_sides(&mut graph);
    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::Label)
            .unwrap(),
    );
    graph.layerless_nodes[dummy].position = LPoint { x: 20.0, y: 40.0 };
    graph.layerless_nodes[dummy].size.height = 14.0;
    graph.layerless_nodes[dummy]
        .label_dummy_mut()
        .unwrap()
        .represented_labels
        .push(LLabel {
            text: "short".to_string(),
            size: LSize {
                width: 12.0,
                height: 4.0,
            },
            position: LPoint::default(),
            placement: EdgeLabelPlacement::Center,
            inline: true,
            label_side: None,
            end_label_edge: None,
            original_label_edge: None,
            source_index: None,
            node_label_placement: None,
        });

    remove_label_dummies(&mut graph);

    let labels = &graph.edges[edge_index].labels;
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].position.y, 41.0);
    assert_eq!(labels[1].position.y, 45.0);
}

#[test]
fn long_edge_joiner_merges_split_segments_and_removes_dummy_from_layer() {
    let mut graph = graph(
        vec![node("A"), node("B"), node("C")],
        vec![
            edge("A-B", "A", "B"),
            edge("B-C", "B", "C"),
            edge("A-C", "A", "C"),
        ],
    );
    graph.options.unnecessary_bendpoints = true;
    layer_network_simplex(&mut graph).unwrap();

    let long_edge = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-C")
            .unwrap(),
    );
    split_long_edges(&mut graph);

    let dummy = NodeId::new(
        graph
            .layerless_nodes
            .iter()
            .position(|node| node.kind() == LNodeKind::LongEdge)
            .unwrap(),
    );
    graph.layerless_nodes[dummy].position = LPoint { x: 50.0, y: 70.0 };
    graph.edges[long_edge]
        .bend_points
        .push(LPoint { x: 10.0, y: 20.0 });
    let dropped = graph
        .edges_with_ids()
        .find(|(index, edge)| *index != long_edge && edge.id == "A-C")
        .map(|(index, _)| index)
        .unwrap();
    graph.edges[dropped]
        .bend_points
        .push(LPoint { x: 90.0, y: 100.0 });
    let mut label = LLabel::new("head", 20.0, 10.0);
    label.placement = EdgeLabelPlacement::Head;
    graph.edges[dropped].labels.push(label);
    let original_target = graph.edges[dropped].target();
    let tail_edge = graph
        .edges_with_ids()
        .find(|(index, edge)| *index != long_edge && *index != dropped && edge.id == "A-B")
        .map(|(index, _)| index)
        .unwrap();
    graph.set_edge_target(tail_edge, original_target);
    assert!(graph.reorder_incoming_edges(original_target, vec![dropped, tail_edge]));

    join_long_edges(&mut graph);

    assert_eq!(graph.edges[long_edge].target(), original_target);
    assert!(graph.edge_source_attached(long_edge));
    assert!(graph.edge_target_attached(long_edge));
    assert!(!graph.edge_source_attached(dropped));
    assert!(!graph.edge_target_attached(dropped));
    assert!(
        !graph
            .layers()
            .iter()
            .any(|layer| layer.nodes().contains(&dummy))
    );
    assert_eq!(
        graph.edges[long_edge].bend_points,
        vec![
            LPoint { x: 10.0, y: 20.0 },
            LPoint { x: 50.0, y: 70.0 },
            LPoint { x: 90.0, y: 100.0 }
        ]
    );
    assert_eq!(graph.edges[long_edge].labels.len(), 1);
    assert!(graph.edges[dropped].labels.is_empty());
    assert_eq!(
        graph.layerless_nodes[original_target.node].ports[original_target.port].incoming_edges(),
        vec![long_edge, tail_edge]
    );
}

#[test]
fn long_edge_joiner_keeps_target_slots_and_order_with_fanout() {
    for pair_count in [1usize, 8, 64] {
        let mut fixture = long_edge_join_fanout_fixture(pair_count);
        let retained_layer_nodes = fixture.graph.layers()[0].nodes().to_vec();

        join_long_edges(&mut fixture.graph);

        let expected_incoming = fixture
            .target_slot_survivors
            .iter()
            .zip(&fixture.guard_edges)
            .flat_map(|(&surviving, &guard)| [surviving, guard])
            .collect::<Vec<_>>();
        assert_eq!(
            fixture.graph.layerless_nodes[fixture.target.node].ports[fixture.target.port]
                .incoming_edges(),
            expected_incoming
        );
        assert!(
            !fixture
                .graph
                .layers()
                .iter()
                .any(|layer| layer.nodes().contains(&fixture.dummy))
        );
        assert_eq!(fixture.graph.layers()[0].nodes(), retained_layer_nodes);

        for pair in &fixture.joined_pairs {
            let surviving = pair.surviving_edge;
            let dropped = pair.dropped_edge;
            assert_eq!(fixture.graph.edges[surviving].target(), fixture.target);
            assert!(fixture.graph.edge_source_attached(surviving));
            assert!(fixture.graph.edge_target_attached(surviving));
            assert!(!fixture.graph.edge_source_attached(dropped));
            assert!(!fixture.graph.edge_target_attached(dropped));
            assert_eq!(
                fixture.graph.edges[surviving].bend_points,
                vec![
                    LPoint {
                        x: pair.surviving_index as f64,
                        y: 10.0,
                    },
                    LPoint { x: 50.0, y: 70.0 },
                    LPoint {
                        x: pair.dropped_index as f64,
                        y: 20.0,
                    },
                    LPoint {
                        x: pair.dropped_index as f64,
                        y: 30.0,
                    },
                ]
            );
            // `LongEdgeJoiner` appends the junction points of the dropped edge, unchanged.
            assert_eq!(
                fixture.graph.edges[surviving].junction_points,
                vec![
                    LPoint {
                        x: pair.surviving_index as f64,
                        y: 11.0,
                    },
                    LPoint {
                        x: pair.dropped_index as f64,
                        y: 21.0,
                    },
                    LPoint {
                        x: pair.dropped_index as f64,
                        y: 31.0,
                    },
                ]
            );
            assert_eq!(
                fixture.graph.edges[surviving]
                    .labels
                    .iter()
                    .map(|label| label.text.clone())
                    .collect::<Vec<_>>(),
                vec![
                    format!("drop-{}-a", pair.dropped_index),
                    format!("drop-{}-b", pair.dropped_index),
                ]
            );
            assert!(fixture.graph.edges[dropped].labels.is_empty());
        }
    }
}

#[test]
fn long_edge_joiner_leaves_layers_alone_when_no_dummy_exists() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
    layer_network_simplex(&mut graph).unwrap();
    let expected_layers = graph.layers().to_vec();

    join_long_edges(&mut graph);

    assert_eq!(graph.layers(), expected_layers.as_slice());
}

#[test]
fn long_edge_joiner_recovers_exact_target_slots_from_a_stale_index() {
    let mut fixture = long_edge_join_fanout_fixture(8);
    let mut stale_positions = HashMap::new();

    join_long_edge_at_indexed(
        &mut fixture.graph,
        fixture.dummy,
        true,
        Some(&mut stale_positions),
    );

    let target_incoming = &fixture.graph.layerless_nodes[fixture.target.node].ports
        [fixture.target.port]
        .incoming_edges();
    let expected_incoming = fixture
        .target_slot_survivors
        .iter()
        .zip(&fixture.guard_edges)
        .flat_map(|(&surviving, &guard)| [surviving, guard])
        .collect::<Vec<_>>();
    assert_eq!(target_incoming, &expected_incoming);
}

// Mutation: in `preprocess_layer_constraints`, do not push the hidden node to `hidden_nodes`.
#[test]
fn layer_constraint_preprocessor_hides_separate_nodes_before_layering() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut graph = graph(
        vec![first, node("A"), node("B")],
        vec![edge("start-A", "start", "A"), edge("A-B", "A", "B")],
    );
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "start-A")
            .unwrap(),
    );

    preprocess_layer_constraints(&mut graph).unwrap();

    let hidden = graph.node_named("start");
    let a = graph.node_named("A");
    assert!(graph.hidden_mask()[hidden]);
    assert_eq!(graph.hidden_nodes, vec![hidden]);
    assert_eq!(
        graph.layerless_nodes[a].layer_constraint,
        LayerConstraint::None
    );
    assert!(
        !graph.edge_target_attached(edge_index),
        "preprocessor should detach the opposite target endpoint"
    );
    assert!(graph.edge_source_attached(edge_index));
}

#[test]
fn layer_constraint_preprocessor_constrains_isolated_opposite_node() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut graph = graph(vec![first, node("A")], vec![edge("start-A", "start", "A")]);

    preprocess_layer_constraints(&mut graph).unwrap();

    let a = graph.node_named("A");
    assert_eq!(
        graph.layerless_nodes[a].layer_constraint,
        LayerConstraint::First
    );
}

#[test]
fn layer_constraint_preprocessor_keeps_explicit_opposite_constraint() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut explicit = node("A");
    explicit.layer_constraint = Some(LayerConstraint::None);
    let mut graph = graph(vec![first, explicit], vec![edge("start-A", "start", "A")]);

    preprocess_layer_constraints(&mut graph).unwrap();

    let a = graph.node_named("A");
    assert!(graph.is_layer_constraint_explicit(a));
    assert_eq!(
        graph.layerless_nodes[a].layer_constraint,
        LayerConstraint::None
    );
}

#[test]
fn layer_constraint_preprocessor_keeps_node_connected_to_both_separate_sides_unconstrained() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut last = node("end");
    last.layer_constraint = Some(LayerConstraint::LastSeparate);
    let mut graph = graph(
        vec![first, node("A"), last],
        vec![edge("start-A", "start", "A"), edge("A-end", "A", "end")],
    );

    preprocess_layer_constraints(&mut graph).unwrap();

    let a = graph.node_named("A");
    assert_eq!(
        graph.layerless_nodes[a].layer_constraint,
        LayerConstraint::None
    );
}

// Mutation: in `restore_hidden_layer_constraint_nodes`, keep `hidden_nodes` as it is.
#[test]
fn layer_constraint_postprocessor_restores_hidden_nodes_in_separate_layer() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut graph = graph(
        vec![first, node("A"), node("B")],
        vec![edge("start-A", "start", "A"), edge("A-B", "A", "B")],
    );
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "start-A")
            .unwrap(),
    );

    preprocess_layer_constraints(&mut graph).unwrap();
    layer_network_simplex(&mut graph).unwrap();
    postprocess_layer_constraints(&mut graph).unwrap();

    let start = graph.node_named("start");
    let a = graph.node_named("A");
    assert!(!graph.hidden_mask()[start]);
    assert!(graph.hidden_nodes.is_empty());
    assert_eq!(graph.layerless_nodes[start].layer_index(), Some(0));
    assert!(
        graph.layerless_nodes[a].layer_index().unwrap()
            > graph.layerless_nodes[start].layer_index().unwrap()
    );
    assert!(graph.edge_source_attached(edge_index));
    assert!(graph.edge_target_attached(edge_index));
}

#[test]
fn layer_constraint_postprocessor_restores_hidden_incoming_edge_source() {
    let mut last = node("end");
    last.layer_constraint = Some(LayerConstraint::LastSeparate);
    let mut graph = graph(vec![node("A"), last], vec![edge("A-end", "A", "end")]);
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id == "A-end")
            .unwrap(),
    );

    preprocess_layer_constraints(&mut graph).unwrap();
    assert!(!graph.edge_source_attached(edge_index));
    assert!(graph.edge_target_attached(edge_index));

    layer_network_simplex(&mut graph).unwrap();
    postprocess_layer_constraints(&mut graph).unwrap();

    let end = graph.node_named("end");
    assert_eq!(
        graph.layerless_nodes[end].layer_index(),
        Some(graph.layers().len() - 1)
    );
    assert!(graph.edge_source_attached(edge_index));
    assert!(graph.edge_target_attached(edge_index));
}

#[test]
fn layer_constraint_preprocessor_rejects_invalid_separate_edges() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut graph = graph(vec![node("A"), first], vec![edge("A-start", "A", "start")]);

    let err = preprocess_layer_constraints(&mut graph).unwrap_err();
    assert!(matches!(
        err,
        IntermediateError::FirstSeparateIncomingEdge { .. }
    ));
}

#[test]
fn layer_constraint_preprocessor_allows_external_port_incident_exception() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::FirstSeparate);
    let mut graph = graph(vec![node("A"), first], vec![edge("A-start", "A", "start")]);
    for node in &mut graph.layerless_nodes {
        node.set_kind(LNodeKind::ExternalPort);
    }

    preprocess_layer_constraints(&mut graph).unwrap();
}

#[test]
fn layer_constraint_postprocessor_rejects_invalid_first_incoming_edge() {
    let mut first = node("start");
    first.layer_constraint = Some(LayerConstraint::First);
    let mut graph = graph(vec![node("A"), first], vec![edge("A-start", "A", "start")]);
    layer_network_simplex(&mut graph).unwrap();

    let err = postprocess_layer_constraints(&mut graph).unwrap_err();
    assert!(matches!(err, IntermediateError::FirstIncomingEdge { .. }));
}

#[test]
fn layer_size_calculator_sets_layer_sizes_graph_height_and_vertical_offset() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::default(),
        nodes: vec![node("A"), node("B")],
        edges: Vec::new(),
    })
    .unwrap();
    graph.layerless_nodes[NodeId::new(0)].size.width = 30.0;
    graph.layerless_nodes[NodeId::new(0)].size.height = 10.0;
    graph.layerless_nodes[NodeId::new(0)].position.y = -5.0;
    graph.layerless_nodes[NodeId::new(0)].margin.top = 3.0;
    graph.layerless_nodes[NodeId::new(0)].margin.right = 7.0;
    graph.layerless_nodes[NodeId::new(0)].margin.bottom = 11.0;
    graph.layerless_nodes[NodeId::new(0)].margin.left = 5.0;
    graph.layerless_nodes[NodeId::new(1)].size.width = 40.0;
    graph.layerless_nodes[NodeId::new(1)].size.height = 20.0;
    graph.layerless_nodes[NodeId::new(1)].position.y = 25.0;
    graph.layerless_nodes[NodeId::new(1)].margin.top = 2.0;
    graph.layerless_nodes[NodeId::new(1)].margin.right = 3.0;
    graph.layerless_nodes[NodeId::new(1)].margin.bottom = 4.0;
    graph.layerless_nodes[NodeId::new(1)].margin.left = 1.0;
    let layer_index = graph.layers().len();
    graph.set_layer_nodes(layer_index, vec![NodeId::new(0), NodeId::new(1)]);

    calculate_layer_sizes_and_graph_height(&mut graph);

    assert_eq!(graph.layers()[0].size.width, 44.0);
    assert_eq!(graph.layers()[0].size.height, 57.0);
    assert_eq!(graph.size.height, 57.0);
    assert_eq!(graph.offset.y, 8.0);
}

#[test]
fn layer_size_calculator_includes_surrounding_spacing_for_external_port_bounds() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::default(),
        nodes: vec![node("port")],
        edges: Vec::new(),
    })
    .unwrap();
    graph.options.spacing.ports_surrounding.top = 6.0;
    graph.options.spacing.ports_surrounding.bottom = 8.0;
    graph.layerless_nodes[NodeId::new(0)].set_kind(LNodeKind::ExternalPort);
    graph.layerless_nodes[NodeId::new(0)].size.height = 12.0;
    graph.layerless_nodes[NodeId::new(0)].position.y = 10.0;
    graph.layerless_nodes[NodeId::new(0)].margin.top = 1.0;
    graph.layerless_nodes[NodeId::new(0)].margin.bottom = 2.0;
    let layer_index = graph.layers().len();
    graph.set_layer_nodes(layer_index, vec![NodeId::new(0)]);

    calculate_layer_sizes_and_graph_height(&mut graph);

    assert_eq!(graph.layers()[0].size.height, 29.0);
    assert_eq!(graph.size.height, 29.0);
    assert_eq!(graph.offset.y, -3.0);
}

#[test]
fn hierarchical_port_constraint_processor_orders_east_west_dummies() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedOrder;
    let west_late = push_external_dummy(&mut graph, "west-late", PortSide::West, 0.8);
    let west_early = push_external_dummy(&mut graph, "west-early", PortSide::West, 0.2);
    let normal = push_normal_node(&mut graph, "A");

    graph.set_node_layer(west_late, 0);
    graph.set_node_layer(west_early, 0);
    graph.set_node_layer(normal, 0);

    process_hierarchical_port_constraints(&mut graph);

    assert_eq!(
        graph.in_layer_successor_constraints(west_early),
        vec![west_late]
    );
    assert!(graph.in_layer_successor_constraints(west_late).is_empty());
}

#[test]
fn hierarchical_port_constraint_processor_replaces_north_south_dummies() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedPos;
    let north = push_external_dummy(&mut graph, "north", PortSide::North, 5.0);
    let a = push_normal_node(&mut graph, "A");
    let b = push_normal_node(&mut graph, "B");

    graph.set_node_layer(a, 0);
    graph.set_node_layer(north, 0);
    graph.set_node_layer(b, 1);

    let in_edge = add_test_edge(&mut graph, "north-A", north, 0, a, 0);
    let out_edge = add_test_edge(&mut graph, "B-north", b, 0, north, 0);

    process_hierarchical_port_constraints(&mut graph);

    assert_eq!(graph.layerless_nodes[north].layer_index(), None);
    assert_eq!(graph.replaced_external_port_dummies, vec![north]);
    let replacements = graph
        .nodes_with_ids()
        .filter_map(|(index, node)| {
            (node
                .external_port()
                .and_then(|external| external.replaced_dummy)
                == Some(north))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(replacements.len(), 2);
    assert_eq!(graph.edges[in_edge].source().node, replacements[0]);
    assert_eq!(graph.edges[in_edge].source().port, PortId::new(1));
    assert_eq!(graph.edges[out_edge].target().node, replacements[1]);
    assert_eq!(graph.edges[out_edge].target().port, PortId::new(0));
    assert!(graph.edge_source_attached(in_edge));
    assert!(graph.edge_target_attached(out_edge));
    assert_eq!(
        graph.layerless_nodes[replacements[0]].port_constraints,
        PortConstraints::FixedPos
    );
    assert_eq!(
        graph.layerless_nodes[replacements[0]].node_alignment,
        Alignment::Center
    );
}

#[test]
fn hierarchical_port_constraint_processor_reuses_north_south_replacements_by_origin_port() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    let first = push_external_dummy(&mut graph, "north-a", PortSide::North, 0.0);
    let second = push_external_dummy(&mut graph, "north-b", PortSide::North, 0.0);
    let a = push_normal_node(&mut graph, "A");
    let b = push_normal_node(&mut graph, "B");

    let origin_port = PortRef {
        node: NodeId::new(42),
        port: PortId::new(7),
    };
    graph.layerless_nodes[first]
        .external_port_mut()
        .unwrap()
        .origin_port = Some(crate::graph::GraphPortRef {
        graph_id: GraphId::new(1),
        port: origin_port,
    });
    graph.layerless_nodes[second]
        .external_port_mut()
        .unwrap()
        .origin_port = Some(crate::graph::GraphPortRef {
        graph_id: GraphId::new(1),
        port: origin_port,
    });

    graph.set_node_layer(first, 0);
    graph.set_node_layer(second, 0);
    graph.set_node_layer(a, 0);
    graph.set_node_layer(b, 0);

    let a_edge = add_test_edge(&mut graph, "north-a-A", first, 0, a, 0);
    let b_edge = add_test_edge(&mut graph, "north-b-B", second, 0, b, 0);

    process_hierarchical_port_constraints(&mut graph);

    let replacements = graph
        .nodes_with_ids()
        .filter_map(|(index, node)| {
            (node
                .external_port()
                .and_then(|external| external.replaced_dummy)
                == Some(first)
                || node
                    .external_port()
                    .and_then(|external| external.replaced_dummy)
                    == Some(second))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(replacements.len(), 1);
    assert_eq!(graph.edges[a_edge].source().node, replacements[0]);
    assert_eq!(graph.edges[b_edge].source().node, replacements[0]);
}

#[test]
fn hierarchical_port_constraint_processor_reuses_north_south_replacements_per_virtual_layer() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    let north_source = push_external_dummy(&mut graph, "north-source", PortSide::North, 0.0);
    let north_target = push_external_dummy(&mut graph, "north-target", PortSide::North, 0.0);
    let middle = push_normal_node(&mut graph, "middle");
    let next = push_normal_node(&mut graph, "next");

    let origin_port = PortRef {
        node: NodeId::new(13),
        port: PortId::new(5),
    };
    for dummy in [north_source, north_target] {
        graph.layerless_nodes[dummy]
            .external_port_mut()
            .unwrap()
            .origin_port = Some(crate::graph::GraphPortRef {
            graph_id: GraphId::new(1),
            port: origin_port,
        });
    }

    graph.set_node_layer(next, 0);
    graph.set_node_layer(north_target, 0);
    graph.set_layer_nodes(graph.layers().len(), vec![]);
    graph.set_node_layer(north_source, 2);
    graph.set_node_layer(middle, 2);

    let incoming_from_north = add_test_edge(
        &mut graph,
        "north-source-middle",
        north_source,
        0,
        middle,
        0,
    );
    let outgoing_to_north =
        add_test_edge(&mut graph, "next-north-target", next, 0, north_target, 0);

    process_hierarchical_port_constraints(&mut graph);

    let replacements = graph
        .nodes_with_ids()
        .filter_map(|(index, node)| {
            (node
                .external_port()
                .and_then(|external| external.replaced_dummy)
                == Some(north_source)
                || node
                    .external_port()
                    .and_then(|external| external.replaced_dummy)
                    == Some(north_target))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(replacements.len(), 1);
    assert_eq!(
        graph.edges[incoming_from_north].source().node,
        replacements[0]
    );
    assert_eq!(
        graph.edges[incoming_from_north].source().port,
        PortId::new(1)
    );
    assert_eq!(
        graph.edges[outgoing_to_north].target().node,
        replacements[0]
    );
    assert_eq!(graph.edges[outgoing_to_north].target().port, PortId::new(0));
}

// Mutation: in `create_hierarchical_port_replacement_dummy`, leave out the copy of the
// successor constraints to the replacement dummy, or of the explicit layer constraint.
#[test]
fn hierarchical_port_constraint_processor_copies_replacement_dummy_metadata() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.42);
    let target = push_normal_node(&mut graph, "target");
    let origin = crate::graph::GraphPortRef {
        graph_id: GraphId::new(1),
        port: PortRef {
            node: NodeId::new(9),
            port: PortId::new(3),
        },
    };
    graph.layerless_nodes[north]
        .external_port_mut()
        .unwrap()
        .origin_port = Some(origin);
    graph.layerless_nodes[north].model_order = Some(17);
    graph.layerless_nodes[north]
        .external_port_mut()
        .unwrap()
        .in_layer_constraint = InLayerConstraint::Top;

    graph
        .tables
        .in_layer
        .row_mut(north)
        .successor_constraints
        .push(target);
    graph.mark_layer_constraint_explicit(north);

    graph.set_node_layer(north, 0);
    graph.set_node_layer(target, 0);
    add_test_edge(&mut graph, "north-target", north, 0, target, 0);

    process_hierarchical_port_constraints(&mut graph);

    let replacement = graph
        .nodes_with_ids()
        .find_map(|(index, node)| {
            (node
                .external_port()
                .and_then(|external| external.replaced_dummy)
                == Some(north))
            .then_some(index)
        })
        .unwrap();
    let node = &graph.layerless_nodes[replacement];
    assert_eq!(graph.in_layer_successor_constraints(replacement), [target]);
    assert!(graph.is_layer_constraint_explicit(replacement));
    assert_eq!(node.external_port().unwrap().ratio_or_position, 0.42);
    assert_eq!(node.external_port().unwrap().origin_port, Some(origin));
    assert_eq!(node.model_order, Some(17));
    assert_eq!(
        node.external_port().unwrap().in_layer_constraint,
        InLayerConstraint::Top
    );
    assert_eq!(node.port_constraints, PortConstraints::FixedPos);
    assert_eq!(node.node_alignment, Alignment::Center);
    assert_eq!(node.ports.len(), 2);
}

#[test]
fn hierarchical_port_dummy_size_processor_sizes_north_south_dummies_per_layer() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.spacing.edge_edge_between_layers = 7.0;

    let north_a = push_replaced_north_south_dummy(&mut graph, "north-a", PortSide::North);
    let north_b = push_replaced_north_south_dummy(&mut graph, "north-b", PortSide::North);
    let south_a = push_replaced_north_south_dummy(&mut graph, "south-a", PortSide::South);
    let south_b = push_replaced_north_south_dummy(&mut graph, "south-b", PortSide::South);

    graph.set_node_layer(north_a, 0);
    graph.set_node_layer(north_b, 0);
    graph.set_node_layer(south_a, 0);
    graph.set_node_layer(south_b, 0);

    process_hierarchical_port_dummy_sizes(&mut graph);

    assert_eq!(
        graph.layerless_nodes[north_a].node_alignment,
        Alignment::Center
    );
    assert_eq!(
        graph.layerless_nodes[north_b].node_alignment,
        Alignment::Center
    );
    assert_eq!(
        graph.layerless_nodes[south_a].node_alignment,
        Alignment::Center
    );
    assert_eq!(
        graph.layerless_nodes[south_b].node_alignment,
        Alignment::Center
    );
    assert_eq!(graph.layerless_nodes[north_a].size.width, 0.0);
    assert_eq!(graph.layerless_nodes[north_b].size.width, 14.0);
    assert_eq!(graph.layerless_nodes[south_a].size.width, 14.0);
    assert_eq!(graph.layerless_nodes[south_b].size.width, 0.0);
    assert_eq!(
        graph.layerless_nodes[north_a].ports[PortId::new(0)]
            .position
            .x,
        0.0
    );
    assert_eq!(
        graph.layerless_nodes[north_b].ports[PortId::new(0)]
            .position
            .x,
        0.0
    );
    assert_eq!(
        graph.layerless_nodes[north_b].ports[PortId::new(1)]
            .position
            .x,
        14.0
    );
    assert_eq!(
        graph.layerless_nodes[south_a].ports[PortId::new(1)]
            .position
            .x,
        14.0
    );
}

#[test]
fn hierarchical_port_position_processor_sets_fixed_ratio_east_west_y_coordinates() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedRatio;
    graph.size.height = 100.0;
    graph.padding.top = 10.0;
    graph.padding.bottom = 20.0;
    graph.offset.y = 7.0;

    let west = push_external_dummy(&mut graph, "west", PortSide::West, 0.25);
    let east = push_external_dummy(&mut graph, "east", PortSide::East, 0.5);
    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.75);
    graph.layerless_nodes[west].ports[PortId::new(0)].position.y = 3.0;
    graph.layerless_nodes[east].ports[PortId::new(0)].position.y = 4.0;
    graph.layerless_nodes[north].ports[PortId::new(0)]
        .position
        .y = 5.0;

    graph.set_node_layer(west, 0);
    graph.set_node_layer(north, 0);
    graph.set_node_layer(east, 1);

    process_hierarchical_port_positions(&mut graph);

    assert_eq!(graph.layerless_nodes[west].position.y, 12.5);
    assert_eq!(graph.layerless_nodes[east].position.y, 44.0);
    assert_eq!(graph.layerless_nodes[north].position.y, 0.0);
}

#[test]
fn hierarchical_port_position_processor_uses_port_position_for_east_west_dummies() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedRatio;
    graph.size.height = 100.0;

    let west = push_external_dummy(&mut graph, "west", PortSide::West, 0.25);
    graph.layerless_nodes[west].ports[PortId::new(0)].position.y = 7.0;
    graph.set_node_layer(west, 0);

    process_hierarchical_port_positions(&mut graph);

    assert_eq!(graph.layerless_nodes[west].position.y, 18.0);
}

#[test]
fn hierarchical_port_position_processor_sets_fixed_pos_only_on_border_layers() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedPos;
    graph.padding.top = 6.0;
    graph.offset.y = -2.0;

    let first = push_external_dummy(&mut graph, "first", PortSide::West, 30.0);
    let middle = push_external_dummy(&mut graph, "middle", PortSide::West, 90.0);
    let last = push_external_dummy(&mut graph, "last", PortSide::East, 50.0);
    graph.layerless_nodes[first].ports[PortId::new(0)]
        .position
        .y = 1.0;
    graph.layerless_nodes[middle].ports[PortId::new(0)]
        .position
        .y = 2.0;
    graph.layerless_nodes[last].ports[PortId::new(0)].position.y = 3.0;

    graph.set_node_layer(first, 0);
    graph.set_node_layer(middle, 1);
    graph.set_node_layer(last, 2);

    process_hierarchical_port_positions(&mut graph);

    assert_eq!(graph.layerless_nodes[first].position.y, 25.0);
    assert_eq!(graph.layerless_nodes[middle].position.y, 0.0);
    assert_eq!(graph.layerless_nodes[last].position.y, 43.0);
}

#[test]
fn hierarchical_port_orthogonal_router_tail_fixes_east_west_coordinates_and_slanted_segments() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedPos;
    graph.size.width = 100.0;
    graph.size.height = 40.0;
    graph.padding.left = 5.0;
    graph.padding.right = 7.0;
    graph.padding.top = 3.0;
    graph.padding.bottom = 11.0;
    graph.offset.x = 2.0;
    graph.offset.y = 4.0;

    let west = push_external_dummy(&mut graph, "west", PortSide::West, 22.0);
    let east = push_external_dummy(&mut graph, "east", PortSide::East, 70.0);
    let middle = push_normal_node(&mut graph, "A");
    graph.layerless_nodes[west]
        .external_port_mut()
        .unwrap()
        .size
        .height = 6.0;
    graph.layerless_nodes[east]
        .external_port_mut()
        .unwrap()
        .size
        .height = 8.0;
    graph.layerless_nodes[west].ports[PortId::new(0)].position.y = 2.0;
    graph.layerless_nodes[east].ports[PortId::new(0)].position.y = 5.0;

    graph.set_node_layer(west, 0);
    graph.set_node_layer(middle, 1);
    graph.set_node_layer(east, 2);

    let west_edge = add_test_edge(&mut graph, "west-A", west, 0, middle, 0);
    graph.edges[west_edge].bend_points = vec![LPoint { x: 10.0, y: 999.0 }];
    let east_edge = add_test_edge(&mut graph, "A-east", middle, 0, east, 0);
    graph.edges[east_edge].bend_points = vec![LPoint { x: 90.0, y: 999.0 }];

    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.layerless_nodes[west].position.x, -7.0);
    assert_eq!(graph.layerless_nodes[east].position.x, 105.0);
    assert_eq!(graph.layerless_nodes[west].position.y, 13.0);
    assert_eq!(graph.layerless_nodes[east].position.y, 58.0);
    assert_eq!(graph.size.height, 59.0);
    assert_eq!(graph.edges[west_edge].bend_points[0].y, 15.0);
    assert_eq!(graph.edges[east_edge].bend_points[0].y, 63.0);
}

#[test]
fn hierarchical_port_orthogonal_router_tail_sets_north_south_border_y_coordinates() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.size.height = 80.0;
    graph.padding.top = 6.0;
    graph.padding.bottom = 9.0;
    graph.offset.y = 4.0;

    let north = push_replaced_north_south_dummy(&mut graph, "north", PortSide::North);
    let south = push_replaced_north_south_dummy(&mut graph, "south", PortSide::South);
    graph.set_node_layer(north, 0);
    graph.set_node_layer(south, 1);

    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.layerless_nodes[north].position.y, -10.0);
    assert_eq!(graph.layerless_nodes[south].position.y, 85.0);
}

#[test]
fn hierarchical_port_orthogonal_router_restores_and_removes_north_dummy_replacements() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    graph.options.spacing.node_node = 20.0;
    graph.options.spacing.edge_edge = 10.0;
    graph.size.height = 50.0;

    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.0);
    graph.layerless_nodes[north].size.width = 4.0;
    graph.layerless_nodes[north].ports[PortId::new(0)].position = LPoint { x: 2.0, y: 0.0 };
    let source = push_normal_node(&mut graph, "A");
    graph.layerless_nodes[source].position = LPoint { x: 50.0, y: 30.0 };
    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::North,
            LPoint { x: 30.0, y: 0.0 },
        )
        .unwrap();

    graph.set_node_layer(source, 0);
    graph.set_node_layer(north, 0);
    let edge = graph
        .add_edge({
            let mut built_edge = LayeredEdge::new(
                "A-north".to_string(),
                source_port,
                PortRef {
                    node: north,
                    port: PortId::new(0),
                },
            );
            built_edge.source_node_id = "A".to_string();
            built_edge.target_node_id = "north".to_string();
            built_edge
        })
        .unwrap();

    process_hierarchical_port_constraints(&mut graph);
    let replacement = graph
        .nodes_with_ids()
        .find_map(|(index, node)| {
            (node
                .external_port()
                .and_then(|external| external.replaced_dummy)
                == Some(north))
            .then_some(index)
        })
        .unwrap();
    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.layerless_nodes[north].layer_index(), Some(1));
    assert_eq!(graph.layerless_nodes[replacement].layer_index(), None);
    assert_eq!(
        graph.layerless_nodes[north].ports[PortId::new(0)].side,
        PortSide::South
    );
    assert_eq!(graph.edges[edge].target().node, north);
    assert_eq!(graph.edges[edge].target().port, PortId::new(0));
    assert!(graph.edge_source_attached(edge));
    assert!(graph.edge_target_attached(edge));
    let detached_edge = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| edge.id.ends_with(":hierarchical-origin"))
            .unwrap(),
    );
    assert!(!graph.edge_source_attached(detached_edge));
    assert!(!graph.edge_target_attached(detached_edge));
}

// Mutation: name the replaced dummy's port `PortId::new(0)` (the first port added) instead of
// the first port of its list, in `remove_temporary_north_south_dummy` or in
// `connect_replacement_dummy_to_original`.
#[test]
fn hierarchical_port_orthogonal_router_uses_the_first_listed_port_of_a_reordered_dummy() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    graph.options.spacing.node_node = 20.0;
    graph.options.spacing.edge_edge = 10.0;
    graph.size.height = 50.0;

    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.0);
    graph.layerless_nodes[north].size.width = 4.0;
    let source = push_normal_node(&mut graph, "A");
    graph.layerless_nodes[source].position = LPoint { x: 50.0, y: 30.0 };
    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::North,
            LPoint { x: 30.0, y: 0.0 },
        )
        .unwrap();
    graph.set_node_layer(source, 0);
    graph.set_node_layer(north, 0);
    let edge = add_test_edge(
        &mut graph,
        "A-north",
        source,
        source_port.port.index(),
        north,
        0,
    );

    process_hierarchical_port_constraints(&mut graph);

    // The dummy lists a second port first. The first port added is no longer the first one.
    let added_first = graph.layerless_nodes[north].ports.id_at(0).unwrap();
    let listed_first = graph
        .add_port(north, PortType::Input, PortSide::West, Default::default())
        .unwrap()
        .port;
    assert!(
        graph.layerless_nodes[north]
            .ports
            .set_order(vec![listed_first, added_first])
    );

    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(
        graph.edges[edge].target(),
        PortRef {
            node: north,
            port: listed_first
        }
    );
    let origin_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id.ends_with(":hierarchical-origin"))
        .unwrap();
    assert_eq!(origin_edge.target().port, listed_first);
}

/// A graph whose north external port dummy has a replacement, as
/// `process_hierarchical_port_constraints` makes it. Returns the graph and the dummy.
fn graph_with_a_replaced_north_dummy() -> (LGraph, NodeId) {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedSide;
    graph.options.spacing.node_node = 20.0;
    graph.options.spacing.edge_edge = 10.0;
    graph.size.height = 50.0;

    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.0);
    graph.layerless_nodes[north].size.width = 4.0;
    let source = push_normal_node(&mut graph, "A");
    graph.layerless_nodes[source].position = LPoint { x: 50.0, y: 30.0 };
    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::North,
            LPoint { x: 30.0, y: 0.0 },
        )
        .unwrap();
    graph.set_node_layer(source, 0);
    graph.set_node_layer(north, 0);
    add_test_edge(
        &mut graph,
        "A-north",
        source,
        source_port.port.index(),
        north,
        0,
    );
    process_hierarchical_port_constraints(&mut graph);
    (graph, north)
}

// Mutation: return without a word from `connect_replacement_dummy_to_original` when the
// replaced dummy lists no port.
#[test]
#[should_panic(expected = "a replaced external port dummy lists at least one port")]
fn connecting_a_replacement_to_a_dummy_without_ports_fails_loudly() {
    let (mut graph, north) = graph_with_a_replaced_north_dummy();
    let replacement = graph
        .node_ids()
        .find(|node| {
            graph.layerless_nodes[*node]
                .external_port()
                .is_some_and(|external| external.replaced_dummy == Some(north))
        })
        .unwrap();
    graph.layerless_nodes[north].ports.clear();

    connect_replacement_dummy_to_original(&mut graph, replacement, north);
}

// Mutation: return without a word from `restore_north_south_dummy` when the dummy lists no port.
#[test]
#[should_panic(expected = "a restored external port dummy lists at least one port")]
fn restoring_a_dummy_without_ports_fails_loudly() {
    let (mut graph, north) = graph_with_a_replaced_north_dummy();
    graph.layerless_nodes[north].ports.clear();

    process_hierarchical_port_orthogonal_edges(&mut graph);
}

// Mutation: return without a word from `remove_temporary_north_south_dummy` when the replaced
// dummy lists no port. The connect step runs first, with the port, because it makes the edge
// the removal looks for.
#[test]
#[should_panic(expected = "a replaced external port dummy lists at least one port")]
fn removing_a_replacement_of_a_dummy_without_ports_fails_loudly() {
    let (mut graph, north) = graph_with_a_replaced_north_dummy();
    restore_north_south_dummies(&mut graph);
    graph.layerless_nodes[north].ports.clear();

    remove_temporary_north_south_dummies(&mut graph);
}

#[test]
fn hierarchical_port_orthogonal_router_places_fixed_ratio_north_south_dummies() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedRatio;
    graph.size.width = 100.0;
    graph.padding.left = 10.0;
    graph.padding.right = 30.0;
    graph.offset.x = 4.0;

    let north = push_external_dummy(&mut graph, "north", PortSide::North, 0.25);
    let south = push_external_dummy(&mut graph, "south", PortSide::South, 0.5);
    graph.layerless_nodes[north].ports[PortId::new(0)]
        .position
        .x = 2.0;
    graph.layerless_nodes[south].ports[PortId::new(0)]
        .position
        .x = 3.0;
    graph.replaced_external_port_dummies = vec![north, south];
    graph.set_layer_nodes(graph.layers().len(), vec![]);

    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.layerless_nodes[north].position.x, 19.0);
    assert_eq!(graph.layerless_nodes[south].position.x, 53.0);
}

#[test]
fn hierarchical_port_orthogonal_router_routes_fixed_pos_north_edges_above_graph() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedPos;
    graph.options.spacing.node_node = 20.0;
    graph.options.spacing.edge_edge = 10.0;
    graph.size.height = 50.0;

    let north = push_external_dummy(&mut graph, "north", PortSide::North, 5.0);
    graph.layerless_nodes[north].size.width = 4.0;
    graph.layerless_nodes[north].ports[PortId::new(0)].position = LPoint { x: 2.0, y: 0.0 };
    let source = push_normal_node(&mut graph, "A");
    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::North,
            LPoint { x: 30.0, y: 0.0 },
        )
        .unwrap();

    graph.set_node_layer(source, 0);
    graph.set_node_layer(north, 0);
    graph
        .add_edge({
            let mut built_edge = LayeredEdge::new(
                "A-north".to_string(),
                source_port,
                PortRef {
                    node: north,
                    port: PortId::new(0),
                },
            );
            built_edge.source_node_id = "A".to_string();
            built_edge.target_node_id = "north".to_string();
            built_edge
        })
        .unwrap();

    process_hierarchical_port_constraints(&mut graph);
    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.offset.y, 20.0);
    assert!(graph.size.height > 50.0);
    let origin_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id.ends_with(":hierarchical-origin"))
        .unwrap();
    assert!(!origin_edge.bend_points.is_empty());
}

#[test]
fn hierarchical_port_orthogonal_router_restores_fixed_order_north_south_spacing() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.port_constraints = PortConstraints::FixedOrder;
    graph.options.spacing.port_port = 7.0;

    let late = push_external_dummy(&mut graph, "late", PortSide::North, 0.8);
    let early = push_external_dummy(&mut graph, "early", PortSide::North, 0.2);
    graph.layerless_nodes[late].position.x = 0.0;
    graph.layerless_nodes[early].position.x = 0.0;
    graph.layerless_nodes[late].size.width = 5.0;
    graph.layerless_nodes[early].size.width = 5.0;
    graph.replaced_external_port_dummies = vec![late, early];
    graph.set_layer_nodes(graph.layers().len(), vec![]);

    process_hierarchical_port_orthogonal_edges(&mut graph);

    assert_eq!(graph.layerless_nodes[early].position.x, 0.0);
    assert_eq!(graph.layerless_nodes[late].position.x, 12.0);
}

fn push_replaced_north_south_dummy(
    graph: &mut LGraph,
    id: &str,
    external_side: PortSide,
) -> NodeId {
    let node = graph.next_node_id();
    let mut dummy = LNode::new(id, 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::ExternalPort);
    dummy.external_port_mut().unwrap().side = external_side;
    graph.layerless_nodes.push(dummy);
    graph
        .add_port(node, PortType::Input, PortSide::West, Default::default())
        .unwrap();
    graph
        .add_port(node, PortType::Output, PortSide::East, Default::default())
        .unwrap();
    node
}

fn push_external_dummy(
    graph: &mut LGraph,
    id: &str,
    external_side: PortSide,
    ratio_or_position: f64,
) -> NodeId {
    let node = graph.next_node_id();
    let mut dummy = LNode::new(id, 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::ExternalPort);
    dummy.external_port_mut().unwrap().side = external_side;
    dummy.external_port_mut().unwrap().ratio_or_position = ratio_or_position;
    graph.layerless_nodes.push(dummy);
    graph
        .add_port(node, PortType::Input, PortSide::West, Default::default())
        .unwrap();
    node
}

fn push_normal_node(graph: &mut LGraph, id: &str) -> NodeId {
    let node = graph.next_node_id();
    graph.layerless_nodes.push(LNode::new(id, 80.0, 40.0, None));
    graph
        .add_port(node, PortType::Input, PortSide::West, Default::default())
        .unwrap();
    node
}

fn add_test_edge(
    graph: &mut LGraph,
    id: &str,
    source_node: NodeId,
    source_port: usize,
    target_node: NodeId,
    target_port: usize,
) -> EdgeId {
    graph
        .add_edge({
            let mut built_edge = LayeredEdge::new(
                id.to_string(),
                PortRef {
                    node: source_node,
                    port: PortId::new(source_port),
                },
                PortRef {
                    node: target_node,
                    port: PortId::new(target_port),
                },
            );
            built_edge.source_node_id = graph.layerless_nodes[source_node].id.clone();
            built_edge.target_node_id = graph.layerless_nodes[target_node].id.clone();
            built_edge
        })
        .unwrap()
}

struct LongEdgeJoinFanoutFixture {
    graph: LGraph,
    dummy: NodeId,
    target: PortRef,
    joined_pairs: Vec<ExpectedLongEdgeJoinPair>,
    target_slot_survivors: Vec<EdgeId>,
    guard_edges: Vec<EdgeId>,
}

struct ExpectedLongEdgeJoinPair {
    surviving_edge: EdgeId,
    dropped_edge: EdgeId,
    surviving_index: usize,
    dropped_index: usize,
}

fn long_edge_join_fanout_fixture(pair_count: usize) -> LongEdgeJoinFanoutFixture {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.unnecessary_bendpoints = true;

    let dummy = graph.next_node_id();
    let mut dummy_node = LNode::new("long-edge", 0.0, 0.0, None);
    dummy_node.set_kind(LNodeKind::LongEdge);
    dummy_node.position = LPoint { x: 50.0, y: 70.0 };
    graph.layerless_nodes.push(dummy_node);
    let dummy_input = graph
        .add_port(dummy, PortType::Input, PortSide::West, LPoint::default())
        .unwrap();
    let dummy_output = graph
        .add_port(dummy, PortType::Output, PortSide::East, LPoint::default())
        .unwrap();

    let target_node = push_normal_node(&mut graph, "target");
    let target = PortRef {
        node: target_node,
        port: PortId::new(0),
    };

    let mut surviving_edges = Vec::with_capacity(pair_count);
    let mut dropped_edges = Vec::with_capacity(pair_count);
    let mut guard_edges = Vec::with_capacity(pair_count);
    for pair_index in 0..pair_count {
        let source = push_output_node(&mut graph, &format!("source-{pair_index}"));
        let guard_source = push_output_node(&mut graph, &format!("guard-source-{pair_index}"));
        graph.set_node_layer(source, 0);
        graph.set_node_layer(guard_source, 0);

        let surviving = add_test_edge(
            &mut graph,
            &format!("surviving-{pair_index}"),
            source,
            0,
            dummy_input.node,
            dummy_input.port.index(),
        );
        graph.edges[surviving].bend_points.push(LPoint {
            x: pair_index as f64,
            y: 10.0,
        });
        graph.edges[surviving].junction_points.push(LPoint {
            x: pair_index as f64,
            y: 11.0,
        });
        surviving_edges.push(surviving);

        let dropped = add_test_edge(
            &mut graph,
            &format!("dropped-{pair_index}"),
            dummy_output.node,
            dummy_output.port.index(),
            target.node,
            target.port.index(),
        );
        graph.edges[dropped].bend_points.extend([
            LPoint {
                x: pair_index as f64,
                y: 20.0,
            },
            LPoint {
                x: pair_index as f64,
                y: 30.0,
            },
        ]);
        graph.edges[dropped].junction_points.extend([
            LPoint {
                x: pair_index as f64,
                y: 21.0,
            },
            LPoint {
                x: pair_index as f64,
                y: 31.0,
            },
        ]);
        graph.edges[dropped]
            .labels
            .push(LLabel::new(format!("drop-{pair_index}-a"), 10.0, 5.0));
        graph.edges[dropped]
            .labels
            .push(LLabel::new(format!("drop-{pair_index}-b"), 10.0, 5.0));
        dropped_edges.push(dropped);

        let guard = add_test_edge(
            &mut graph,
            &format!("guard-{pair_index}"),
            guard_source,
            0,
            target.node,
            target.port.index(),
        );
        guard_edges.push(guard);
    }

    let mut input_order = surviving_edges
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();
    input_order.reverse();
    let mut output_order = dropped_edges
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();
    output_order.rotate_left(pair_count / 3);
    assert!(graph.reorder_incoming_edges(
        dummy_input,
        input_order.iter().map(|(_, edge)| *edge).collect()
    ));
    assert!(graph.reorder_outgoing_edges(
        dummy_output,
        output_order.iter().map(|(_, edge)| *edge).collect()
    ));

    let mut target_slot_survivors = vec![EdgeId::new(usize::MAX); pair_count];
    let joined_pairs = input_order
        .into_iter()
        .zip(output_order)
        .map(
            |((surviving_index, surviving_edge), (dropped_index, dropped_edge))| {
                target_slot_survivors[dropped_index] = surviving_edge;
                ExpectedLongEdgeJoinPair {
                    surviving_edge,
                    dropped_edge,
                    surviving_index,
                    dropped_index,
                }
            },
        )
        .collect();

    graph.set_node_layer(dummy, 1);
    graph.set_node_layer(target.node, 2);

    LongEdgeJoinFanoutFixture {
        graph,
        dummy,
        target,
        joined_pairs,
        target_slot_survivors,
        guard_edges,
    }
}

fn push_output_node(graph: &mut LGraph, id: &str) -> NodeId {
    let node = graph.next_node_id();
    graph.layerless_nodes.push(LNode::new(id, 80.0, 40.0, None));
    graph
        .add_port(node, PortType::Output, PortSide::East, LPoint::default())
        .unwrap();
    node
}
