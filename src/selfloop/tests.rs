//! Tests of the classes in this module.

use crate::graph::{
    EdgeId, LGraph, LPoint, LSize, NodeId, NodePorts, PortId, PortRef, PortSide, SelfHyperLoop,
    SelfHyperLoopLabels, SelfLoopHolder, SelfLoopLabelAlignment, SelfLoopPort, SelfLoopType,
};
use crate::graph::{LLabel, LNode, LayeredEdge, PortType};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::{
    ElkDirection, LayeredOptions, PortConstraints, SelfLoopDistributionStrategy,
    SelfLoopOrderingStrategy,
};
use crate::selfloop::ordering::port_restorer::{AddMode, PortRestoreTargetAreas, PortSideArea};
use crate::selfloop::routing::routing_director::{
    PortPlaces, compute_edge_penalty, determine_loop_routes,
};
use crate::selfloop::routing::routing_slot_assigner::{
    assign_routing_slots, compute_loop_activity,
};
use crate::selfloop::self_loop_port_restorer::restore_self_loop_ports;
use crate::selfloop::self_loop_pre_processor::preprocess_self_loops;
use crate::selfloop::self_loop_router::route_self_loops;

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

fn self_loop_edge(id: &str, node_id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
    layered_edge(id, node_id, node_id, source, target)
}

fn layered_edge(
    id: &str,
    source_node_id: &str,
    target_node_id: &str,
    source: PortRef,
    target: PortRef,
) -> LayeredEdge {
    let mut built_edge = LayeredEdge::new(id.to_string(), source, target);
    built_edge.source_node_id = source_node_id.to_string();
    built_edge.target_node_id = target_node_id.to_string();
    built_edge
}

fn empty_self_loop_holder(node: NodeId, hyper_loop: SelfHyperLoop) -> SelfLoopHolder {
    SelfLoopHolder {
        node,
        hyper_loops: vec![hyper_loop],
        ports_hidden: false,
        hidden_ports: Vec::new(),
        original_port_constraints: PortConstraints::FixedSide,
    }
}

fn self_loop_port(port: PortRef) -> SelfLoopPort {
    SelfLoopPort {
        port: port.port,
        had_only_self_loops: false,
        hidden: false,
    }
}

fn hyper_loop_with_ports(ports: &[PortRef], self_loop_type: SelfLoopType) -> SelfHyperLoop {
    SelfHyperLoop {
        ports: ports.iter().copied().map(self_loop_port).collect(),
        edges: Vec::new(),
        labels: None,
        self_loop_type: Some(self_loop_type),
        leftmost_port: None,
        rightmost_port: None,
        occupied_sides: Vec::new(),
        routing_slots: [0; 5],
    }
}

fn add_node(graph: &mut LGraph, id: &str) -> NodeId {
    let node = graph.next_node_id();
    graph.layerless_nodes.push(LNode::new(id, 80.0, 40.0, None));
    node
}

fn add_port(graph: &mut LGraph, node: NodeId, side: PortSide) -> PortRef {
    graph
        .add_port(node, PortType::Output, side, LPoint::default())
        .unwrap()
}

fn connect_to_sink(graph: &mut LGraph, source: PortRef, sink: PortRef, id: &str) {
    graph.add_edge(layered_edge(id, "A", "B", source, sink));
}

#[test]
fn restore_target_areas_match_java_prepend_and_append_ordering() {
    let mut areas = PortRestoreTargetAreas::new();

    areas.add_port_list(
        vec![PortId::new(1), PortId::new(2)],
        PortSide::North,
        PortSideArea::Middle,
        AddMode::Append,
    );
    areas.add_port_list(
        vec![PortId::new(3), PortId::new(4)],
        PortSide::North,
        PortSideArea::Middle,
        AddMode::Prepend,
    );

    assert_eq!(
        areas.get(PortSide::North, PortSideArea::Middle),
        &[4, 3, 2, 1].map(PortId::new)
    );
}

#[test]
fn restore_hidden_one_side_ports_respects_sequenced_ordering() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    graph.tables.self_loop_options.row_mut(node).distribution = SelfLoopDistributionStrategy::North;
    graph.tables.self_loop_options.row_mut(node).ordering = SelfLoopOrderingStrategy::Sequenced;
    let source = add_port(&mut graph, node, PortSide::North);
    let target = add_port(&mut graph, node, PortSide::North);
    graph.add_edge(self_loop_edge("loop", "A", source, target));

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());

    let order = graph.layerless_nodes[node]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["A:1", "A:0"]);
}

#[test]
fn restore_hidden_one_side_ports_respects_reverse_stacked_ordering() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    graph.tables.self_loop_options.row_mut(node).distribution = SelfLoopDistributionStrategy::North;
    graph.tables.self_loop_options.row_mut(node).ordering =
        SelfLoopOrderingStrategy::ReverseStacked;
    let p0 = add_port(&mut graph, node, PortSide::North);
    let p1 = add_port(&mut graph, node, PortSide::North);
    let p2 = add_port(&mut graph, node, PortSide::North);
    let p3 = add_port(&mut graph, node, PortSide::North);
    graph.add_edge(self_loop_edge("loop-1", "A", p0, p1));
    graph.add_edge(self_loop_edge("loop-2", "A", p2, p3));

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());

    let order = graph.layerless_nodes[node]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["A:0", "A:2", "A:3", "A:1"]);
}

// Mutation: start the walk one place after the leftmost port (`leftmost_port as isize`
// instead of `- 1`), which leaves a leftmost port at place 0 out of the loop's activity.
#[test]
fn loop_activity_includes_zero_leftmost_port() {
    let hyper_loop = hyper_loop_with_ports(
        &[
            PortRef {
                node: NodeId::new(0),
                port: PortId::new(0),
            },
            PortRef {
                node: NodeId::new(0),
                port: PortId::new(2),
            },
        ],
        SelfLoopType::OneSide,
    );
    let mut holder = empty_self_loop_holder(NodeId::new(0), hyper_loop);
    holder.hyper_loops[0].leftmost_port = Some(PortId::new(0));
    holder.hyper_loops[0].rightmost_port = Some(PortId::new(2));

    let mut ports = NodePorts::default();
    for index in 0..4 {
        ports.push(crate::graph::LPort::new(
            format!("p{index}"),
            NodeId::new(0),
            crate::graph::PortType::Input,
        ));
    }
    let activity = compute_loop_activity(&holder, &PortPlaces::new(&ports));

    assert_eq!(activity[0], vec![true, true, true, false]);
}

#[test]
fn preprocess_detaches_self_loop_edges_from_ports() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A")],
        edges: vec![edge("A-A", "A", "A")],
    })
    .unwrap();

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());

    assert_eq!(graph.self_loop_holders.len(), 1);
    assert!(!graph.edge_source_attached(EdgeId::new(0)));
    assert!(!graph.edge_target_attached(EdgeId::new(0)));
    assert!(graph.self_loop_holders[0].ports_hidden);
}

#[test]
fn restorer_assigns_first_equal_distribution_loop_to_north_side() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A")],
        edges: vec![edge("A-A", "A", "A")],
    })
    .unwrap();

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());

    assert!(
        graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .all(|port| port.side == PortSide::North)
    );
    assert_eq!(
        graph.self_loop_holders[0].hyper_loops[0].self_loop_type,
        Some(SelfLoopType::OneSide)
    );
}

#[test]
fn restorer_defaults_to_north_distribution_for_multiple_loops() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let first_source = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    let first_target = graph
        .add_port(
            node,
            PortType::Input,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    let second_source = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    let second_target = graph
        .add_port(
            node,
            PortType::Input,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    graph.add_edge(self_loop_edge("first", "A", first_source, first_target));
    graph.add_edge(self_loop_edge("second", "A", second_source, second_target));

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());

    assert!(
        graph.layerless_nodes[node]
            .ports
            .iter()
            .all(|port| port.side == PortSide::North)
    );
}

#[test]
fn restorer_assigns_equal_distribution_targets_by_descending_loop_port_count() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    // The Mermaid adapter default is node-scoped NORTH. Exercise the explicit
    // node strategy here so this test continues to cover EQUALLY assignment.
    let node = add_node(&mut graph, "A");
    graph.tables.self_loop_options.row_mut(node).distribution =
        SelfLoopDistributionStrategy::Equally;
    let single_port = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    let double_source = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    let double_target = graph
        .add_port(
            node,
            PortType::Input,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();

    graph.add_edge(self_loop_edge("single", "A", single_port, single_port));
    graph.add_edge(self_loop_edge("double", "A", double_source, double_target));

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    assert_eq!(graph.self_loop_holders[0].hyper_loops[0].ports.len(), 1);
    assert_eq!(graph.self_loop_holders[0].hyper_loops[1].ports.len(), 2);

    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());

    assert_eq!(
        graph.layerless_nodes[node].ports[graph.edges[EdgeId::new(0)].source().port].side,
        PortSide::South
    );
    assert_eq!(
        graph.layerless_nodes[node].ports[graph.edges[EdgeId::new(1)].source().port].side,
        PortSide::North
    );
    assert_eq!(
        graph.layerless_nodes[node].ports[graph.edges[EdgeId::new(1)].target().port].side,
        PortSide::North
    );
}

// Mutation: index the table by the place of a port instead of by `PortId::index`.
#[test]
fn port_places_follow_the_list_after_a_reorder_and_an_unlist() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let first = add_port(&mut graph, node, PortSide::North).port;
    let second = add_port(&mut graph, node, PortSide::North).port;
    let third = add_port(&mut graph, node, PortSide::North).port;
    let fourth = add_port(&mut graph, node, PortSide::North).port;
    let ports = &mut graph.layerless_nodes[node].ports;
    ports.unlist(&[second]);
    assert!(ports.set_order(vec![fourth, first, third]));

    let places = PortPlaces::new(ports);

    assert_eq!(places.of(fourth), Some(0));
    assert_eq!(places.of(first), Some(1));
    assert_eq!(places.of(third), Some(2));
    assert_eq!(places.of(second), None);
    assert_eq!(places.of(PortId::new(9)), None);
}

// Mutation: treat a port off the list as place 0 instead of as "no place".
#[test]
fn a_port_off_the_list_has_no_penalty_and_no_activity() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let listed = add_port(&mut graph, node, PortSide::North);
    let off_list = add_port(&mut graph, node, PortSide::North);
    let last = add_port(&mut graph, node, PortSide::North);
    graph.layerless_nodes[node].ports.unlist(&[off_list.port]);
    let places = PortPlaces::new(&graph.layerless_nodes[node].ports);

    assert_eq!(
        compute_edge_penalty(&places, off_list.port, last.port, &[4, 9]),
        0
    );
    assert_eq!(
        compute_edge_penalty(&places, listed.port, off_list.port, &[4, 9]),
        0
    );

    let mut hyper_loop = hyper_loop_with_ports(&[listed, off_list], SelfLoopType::OneSide);
    hyper_loop.leftmost_port = Some(off_list.port);
    hyper_loop.rightmost_port = Some(last.port);
    let holder = empty_self_loop_holder(node, hyper_loop);
    let activity = compute_loop_activity(&holder, &places);
    assert_eq!(activity, vec![vec![false, false]]);
}

// Mutation: expect every self-loop port to be in the node's list, and panic when one is not.
#[test]
fn routing_a_loop_with_a_port_off_the_list_does_not_panic() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let listed = add_port(&mut graph, node, PortSide::North);
    let off_list = add_port(&mut graph, node, PortSide::North);
    graph.layerless_nodes[node].ports.unlist(&[off_list.port]);
    let hyper_loop = hyper_loop_with_ports(&[listed, off_list], SelfLoopType::OneSide);
    let mut holder = empty_self_loop_holder(node, hyper_loop);

    determine_loop_routes(
        &graph,
        &mut holder,
        &PortPlaces::new(&graph.layerless_nodes[node].ports),
    );

    // A port off the list has no place, so it sorts before the listed ones.
    let hyper_loop = &holder.hyper_loops[0];
    assert_eq!(hyper_loop.ports[0].port, off_list.port);
    assert_eq!(hyper_loop.leftmost_port, Some(off_list.port));
    assert_eq!(hyper_loop.rightmost_port, Some(listed.port));
}

// Mutation: take option 1 always, whatever the penalties of the two options are.
#[test]
fn routing_director_uses_penalty_for_two_side_opposing_loops() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let sink = add_node(&mut graph, "B");

    let north = add_port(&mut graph, node, PortSide::North);
    let connected_1 = add_port(&mut graph, node, PortSide::East);
    let connected_2 = add_port(&mut graph, node, PortSide::East);
    let south = add_port(&mut graph, node, PortSide::South);
    let west_1 = add_port(&mut graph, node, PortSide::West);
    let west_2 = add_port(&mut graph, node, PortSide::West);
    let sink_1 = add_port(&mut graph, sink, PortSide::West);
    let sink_2 = add_port(&mut graph, sink, PortSide::West);
    connect_to_sink(&mut graph, connected_1, sink_1, "connected-1");
    connect_to_sink(&mut graph, connected_2, sink_2, "connected-2");

    let hyper_loop = hyper_loop_with_ports(&[north, south], SelfLoopType::TwoSidesOpposing);
    let mut holder = empty_self_loop_holder(node, hyper_loop);

    determine_loop_routes(
        &graph,
        &mut holder,
        &PortPlaces::new(&graph.layerless_nodes[node].ports),
    );

    let hyper_loop = &holder.hyper_loops[0];
    assert_eq!(hyper_loop.leftmost_port, Some(south.port));
    assert_eq!(hyper_loop.rightmost_port, Some(north.port));
    assert_eq!(
        hyper_loop.occupied_sides,
        vec![PortSide::South, PortSide::West, PortSide::North]
    );
    assert_eq!(west_1.port, PortId::new(4));
    assert_eq!(west_2.port, PortId::new(5));
}

// Mutation: compare with `<` so the loop splits at the lowest penalty gap.
#[test]
fn routing_director_splits_four_side_loops_at_highest_penalty_gap() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let sink = add_node(&mut graph, "B");

    let north = add_port(&mut graph, node, PortSide::North);
    let connected_1 = add_port(&mut graph, node, PortSide::North);
    let connected_2 = add_port(&mut graph, node, PortSide::North);
    let east = add_port(&mut graph, node, PortSide::East);
    let south = add_port(&mut graph, node, PortSide::South);
    let west = add_port(&mut graph, node, PortSide::West);
    let sink_1 = add_port(&mut graph, sink, PortSide::West);
    let sink_2 = add_port(&mut graph, sink, PortSide::West);
    connect_to_sink(&mut graph, connected_1, sink_1, "connected-1");
    connect_to_sink(&mut graph, connected_2, sink_2, "connected-2");

    let hyper_loop = hyper_loop_with_ports(&[north, east, south, west], SelfLoopType::FourSides);
    let mut holder = empty_self_loop_holder(node, hyper_loop);

    determine_loop_routes(
        &graph,
        &mut holder,
        &PortPlaces::new(&graph.layerless_nodes[node].ports),
    );

    let hyper_loop = &holder.hyper_loops[0];
    assert_eq!(hyper_loop.leftmost_port, Some(east.port));
    assert_eq!(hyper_loop.rightmost_port, Some(north.port));
    assert_eq!(
        hyper_loop.occupied_sides,
        vec![
            PortSide::East,
            PortSide::South,
            PortSide::West,
            PortSide::North
        ]
    );
}

// Mutation: add no dependency between two loops in `create_self_loop_slot_dependencies`.
#[test]
fn routing_slot_assigner_uses_crossing_graph_dependencies() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let p0 = add_port(&mut graph, node, PortSide::North);
    let p1 = add_port(&mut graph, node, PortSide::North);
    let p2 = add_port(&mut graph, node, PortSide::North);
    let p3 = add_port(&mut graph, node, PortSide::North);

    let mut outer = hyper_loop_with_ports(&[p0, p3], SelfLoopType::OneSide);
    outer.leftmost_port = Some(p0.port);
    outer.rightmost_port = Some(p3.port);
    outer.occupied_sides = vec![PortSide::North];
    let mut inner = hyper_loop_with_ports(&[p1, p2], SelfLoopType::OneSide);
    inner.leftmost_port = Some(p1.port);
    inner.rightmost_port = Some(p2.port);
    inner.occupied_sides = vec![PortSide::North];
    let mut holder = SelfLoopHolder {
        node,
        hyper_loops: vec![outer, inner],
        ports_hidden: false,
        hidden_ports: Vec::new(),
        original_port_constraints: PortConstraints::FixedSide,
    };

    let places = PortPlaces::new(&graph.layerless_nodes[holder.node].ports);
    assign_routing_slots(&mut graph, &mut holder, &places);

    assert_eq!(
        holder.hyper_loops[0].routing_slots[PortSide::North.ordinal()],
        1
    );
    assert_eq!(
        holder.hyper_loops[1].routing_slots[PortSide::North.ordinal()],
        0
    );
}

// Mutation: report no overlap between two labels in `self_loop_labels_overlap`.
#[test]
fn routing_slot_assigner_separates_overlapping_self_loop_labels() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let p0 = add_port(&mut graph, node, PortSide::North);
    let p1 = add_port(&mut graph, node, PortSide::North);
    let p2 = add_port(&mut graph, node, PortSide::North);
    let p3 = add_port(&mut graph, node, PortSide::North);

    let mut left = hyper_loop_with_ports(&[p0, p1], SelfLoopType::OneSide);
    left.leftmost_port = Some(p0.port);
    left.rightmost_port = Some(p1.port);
    left.occupied_sides = vec![PortSide::North];
    left.labels = Some(SelfHyperLoopLabels {
        id: None,
        label_refs: Vec::new(),
        size: LSize {
            width: 90.0,
            height: 12.0,
        },
        position: LPoint { x: 0.0, y: 0.0 },
        side: Some(PortSide::North),
        alignment: Some(SelfLoopLabelAlignment::Center),
        alignment_reference_port: None,
    });

    let mut right = hyper_loop_with_ports(&[p2, p3], SelfLoopType::OneSide);
    right.leftmost_port = Some(p2.port);
    right.rightmost_port = Some(p3.port);
    right.occupied_sides = vec![PortSide::North];
    right.labels = Some(SelfHyperLoopLabels {
        id: None,
        label_refs: Vec::new(),
        size: LSize {
            width: 90.0,
            height: 12.0,
        },
        position: LPoint { x: 10.0, y: 0.0 },
        side: Some(PortSide::North),
        alignment: Some(SelfLoopLabelAlignment::Center),
        alignment_reference_port: None,
    });

    let mut holder = SelfLoopHolder {
        node,
        hyper_loops: vec![left, right],
        ports_hidden: false,
        hidden_ports: Vec::new(),
        original_port_constraints: PortConstraints::FixedSide,
    };

    let places = PortPlaces::new(&graph.layerless_nodes[holder.node].ports);
    assign_routing_slots(&mut graph, &mut holder, &places);

    assert_ne!(
        holder.hyper_loops[0].routing_slots[PortSide::North.ordinal()],
        holder.hyper_loops[1].routing_slots[PortSide::North.ordinal()]
    );
}

#[test]
fn route_self_loops_writes_label_positions_and_expands_margins() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    let node = add_node(&mut graph, "A");
    let source = add_port(&mut graph, node, PortSide::North);
    let target = add_port(&mut graph, node, PortSide::North);
    let mut edge = self_loop_edge("loop", "A", source, target);
    edge.labels.push(LLabel::new("loop label", 60.0, 12.0));
    graph.add_edge(edge);

    preprocess_self_loops(&mut graph, crate::graph::OtherGraphs::none());
    restore_self_loop_ports(&mut graph, crate::graph::OtherGraphs::none());
    route_self_loops(&mut graph);

    let label = &graph.edges[EdgeId::new(0)].labels[0];
    assert_eq!(label.position.x, 10.0);
    assert_eq!(label.position.y, -36.0);
    assert!(graph.layerless_nodes[node].margin.top >= 36.0);
}
