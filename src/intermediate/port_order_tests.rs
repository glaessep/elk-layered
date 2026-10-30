//! Tests of the classes in this module.

use std::collections::HashMap;

use crate::graph::{EdgeId, GraphNodeRef, LNode, LNodeKind, LPoint, LayeredEdge, PortType};
use crate::graph::{GraphId, LGraph, NodeId, PortId, PortRef, PortSide};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::intermediate::port_list_sorter::{reverse_side_range, sort_port_lists};
use crate::intermediate::port_side_processor::{process_port_sides, set_port_side};
use crate::intermediate::preserveorder::model_order_node_comparator::last_previous_layer_source_port;
use crate::intermediate::preserveorder::model_order_port_comparator::ModelOrderPortComparator;
use crate::intermediate::sort_by_input_model_processor::{
    long_edge_target_node_preprocessing, sort_by_input_model, target_node,
};
use crate::intermediate::split_long_edges;
use crate::options::{ElkDirection, LayeredOptions};
use crate::options::{OrderingStrategy, PortConstraints};
use crate::phase::layering::layer_network_simplex;
use crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids;
use crate::phase::ordering::sweep_copy::{SweepCopy, correct_north_south_port_sides};

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

fn layered_edge(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
    let mut built_edge = LayeredEdge::new(id.to_string(), source, target);
    built_edge.source_node_id = format!("node-{}", source.node.index());
    built_edge.target_node_id = format!("node-{}", target.node.index());
    built_edge
}

#[test]
fn model_order_sort_matches_pinned_worker_comparison_schedule() {
    // Extracted insertionSort/mergeSort_0/merge_1 from elkjs 0.9.3, including
    // the seven-element merge boundary and the already-ordered fast path.
    type SortCase = (
        &'static [usize],
        &'static [usize],
        &'static [(usize, usize)],
    );
    let cases: &[SortCase] = &[
        (&[], &[], &[]),
        (&[0], &[0], &[]),
        (&[2, 1, 0], &[0, 1, 2], &[(2, 1), (2, 0), (1, 0)]),
        (
            &[6, 5, 4, 3, 2, 1, 0],
            &[0, 1, 2, 3, 4, 5, 6],
            &[
                (6, 5),
                (6, 4),
                (5, 4),
                (3, 2),
                (3, 1),
                (2, 1),
                (3, 0),
                (2, 0),
                (1, 0),
                (6, 0),
                (4, 0),
                (4, 1),
                (4, 2),
                (4, 3),
            ],
        ),
        (
            &[0, 1, 2, 3, 4, 5, 6],
            &[0, 1, 2, 3, 4, 5, 6],
            &[(0, 1), (1, 2), (3, 4), (4, 5), (5, 6), (2, 3)],
        ),
    ];
    for &(input, expected, expected_calls) in cases {
        let mut order = input.to_vec();
        let mut calls = Vec::new();
        crate::gwt::sort(&mut order, |left, right| {
            calls.push((left, right));
            left.cmp(&right)
        });
        assert_eq!(order, expected, "input={input:?}");
        assert_eq!(calls, expected_calls, "input={input:?}");
    }
}

#[test]
fn model_order_sort_preserves_equal_keys_across_recursive_merges() {
    let keys = [2, 1, 2, 0, 1, 0, 2, 1, 2, 0, 1, 0, 2, 1];
    let mut order = (0..keys.len()).collect::<Vec<_>>();
    crate::gwt::sort(&mut order, |left, right| keys[left].cmp(&keys[right]));
    assert_eq!(order, [3, 5, 9, 11, 1, 4, 7, 10, 13, 0, 2, 6, 8, 12]);
}

#[test]
fn model_order_sort_preserves_source_stateful_port_fallback() {
    for length in [2, 6, 7, 14] {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("parent", 0.0, 0.0, None));
        for _ in 0..length {
            graph
                .add_port(
                    NodeId::new(0),
                    PortType::Input,
                    PortSide::West,
                    LPoint::default(),
                )
                .unwrap();
        }
        let target_orders = HashMap::new();
        let mut comparator = ModelOrderPortComparator::new(
            &graph,
            NodeId::new(0),
            &[],
            OrderingStrategy::NodesAndEdges,
            &target_orders,
            false,
        );
        let mut order = (0..length).map(PortId::new).collect::<Vec<_>>();
        crate::gwt::sort(&mut order, |left, right| comparator.compare(left, right));
        assert_eq!(order, (0..length).map(PortId::new).collect::<Vec<_>>());
        // The first source comparison records port 0 < port 1. Reversing
        // the first call (as Rust sort_by can do) records the opposite.
        assert!(comparator.smaller_than[&PortId::new(0)].contains(&PortId::new(1)));
        assert!(comparator.bigger_than[&PortId::new(1)].contains(&PortId::new(0)));
    }
}

// Mutation: add a new port at the front of the node's list, not at its end (`NodePorts::push`
// with `order.insert(0, id)`). The test reads both places with `unwrap`, because a port off
// the list has no place and `None` sorts before every place.
#[test]
fn compound_self_loop_keeps_input_ports_in_source_model_order() {
    let mut parent = node("Active");
    parent.width = 0.0;
    parent.height = 0.0;
    parent.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child = node("Idle");
    child.parent = Some("Active".to_string());
    let mut graph = graph(
        vec![parent, child, node("Inactive")],
        vec![
            edge("edge0", "Inactive", "Idle"),
            edge("edge1", "Active", "Active"),
        ],
    );
    crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();
    let loop_edge = graph.edges.iter().find(|edge| edge.id == "edge1").unwrap();
    let parent_index = loop_edge.target().node;
    let parent = &graph.layerless_nodes[parent_index];
    let cross_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == "edge0" && edge.target().node == parent_index)
        .unwrap();
    // The graph flows downward, so the finished layout has turned east into south and west
    // into north.
    assert_eq!(parent.ports[loop_edge.source().port].side, PortSide::South);
    assert_eq!(parent.ports[loop_edge.target().port].side, PortSide::North);
    assert_eq!(parent.ports[cross_edge.target().port].side, PortSide::North);
    // Both ports must be in the list: `None` would sort below any place.
    let loop_place = parent.ports.position(loop_edge.target().port).unwrap();
    let cross_place = parent.ports.position(cross_edge.target().port).unwrap();
    assert!(loop_place < cross_place);
    // Reordering must also keep each child external port's origin linked
    // to its corresponding parent port, including detached self-loop ports.
    let child = &graph[parent.nested_graph.unwrap()];
    for (index, port) in parent.ports.iter_with_ids() {
        let dummy = port.port_dummy.as_ref().unwrap();
        let origin = child.layerless_nodes[dummy.node]
            .external_port()
            .unwrap()
            .origin_port
            .as_ref()
            .unwrap();
        assert_eq!(
            origin.port,
            PortRef {
                node: parent_index,
                port: index
            }
        );
    }
}

#[test]
fn port_side_processor_assigns_by_net_flow_and_fixes_constraints() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);

    process_port_sides(&mut graph, crate::graph::OtherGraphs::none());

    let a = graph.node_named("A");
    let b = graph.node_named("B");
    assert_eq!(
        graph.layerless_nodes[a].port_constraints,
        PortConstraints::FixedSide
    );
    assert_eq!(
        graph.layerless_nodes[b].port_constraints,
        PortConstraints::FixedSide
    );
    assert_eq!(
        graph.layerless_nodes[a].ports[PortId::new(0)].side,
        PortSide::East
    );
    assert_eq!(
        graph.layerless_nodes[b].ports[PortId::new(0)].side,
        PortSide::West
    );
}

#[test]
fn port_side_processor_uses_external_port_dummy_side_before_net_flow() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let parent = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("parent", 80.0, 40.0, None));
    let dummy = graph.next_node_id();
    let mut external = LNode::new("external:parent", 0.0, 0.0, None);
    external.set_kind(LNodeKind::ExternalPort);
    external.external_port_mut().unwrap().side = PortSide::North;
    graph.layerless_nodes.push(external);
    let port = graph
        .add_port(
            parent,
            PortType::Output,
            PortSide::Undefined,
            LPoint::default(),
        )
        .unwrap();
    graph.layerless_nodes[parent].ports[port.port].port_dummy = Some(GraphNodeRef {
        graph_id: GraphId::ROOT,
        node: dummy,
    });

    set_port_side(&mut graph, crate::graph::OtherGraphs::none(), port);

    assert_eq!(
        graph.layerless_nodes[parent].ports[port.port].side,
        PortSide::North
    );
}

#[test]
fn port_list_sorter_uses_clockwise_order_and_rewrites_edge_refs() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
    let a = graph.node_named("A");
    graph.layerless_nodes[a].port_constraints = PortConstraints::FixedPos;
    let north = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::North,
            LPoint { x: 10.0, y: 0.0 },
        )
        .unwrap();
    let south = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::South,
            LPoint { x: 1.0, y: 0.0 },
        )
        .unwrap();
    graph.layerless_nodes[a].ports[north.port].id = "north".to_string();
    graph.layerless_nodes[a].ports[south.port].id = "south".to_string();
    graph.layerless_nodes[a].ports[PortId::new(0)].set_side(PortSide::West);
    graph.layerless_nodes[a].ports[PortId::new(0)].position = LPoint { x: 0.0, y: 100.0 };
    graph.set_node_layer(a, 0);

    sort_port_lists(&mut graph);

    assert_eq!(
        graph.layerless_nodes[a]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["north", "south", "A:0"]
    );
    assert_eq!(graph.edges[EdgeId::new(0)].source().node, a);
    assert_eq!(
        graph.layerless_nodes[a].ports[graph.edges[EdgeId::new(0)].source().port].id,
        "A:0"
    );
}

// Mutation: reverse the south and west ports one port short of the pinned range.
#[test]
fn port_list_sorter_matches_pinned_side_ranges_and_preserves_edge_endpoints() {
    use PortSide::{East, North, South, West};

    // Expected permutations come from the pinned elkjs 0.9.3 PortListSorter,
    // including its exclusive last-port bound and sequential south/west passes.
    let cases: &[(&[PortSide], &[usize])] = &[
        (&[], &[]),
        (&[West], &[0]),
        (&[West, West], &[0, 1]),
        (&[West, West, West], &[0, 1, 2]),
        (&[West, West, West, West], &[2, 1, 0, 3]),
        (&[West, West, West, West, West], &[3, 2, 1, 0, 4]),
        (&[South], &[0]),
        (&[South, South], &[0, 1]),
        (&[South, South, South], &[0, 1, 2]),
        (&[South, South, South, South], &[2, 1, 0, 3]),
        (&[South, South, South, South, South], &[3, 2, 1, 0, 4]),
        (&[North, North, East, East, East], &[0, 1, 2, 3, 4]),
        (
            &[North, East, South, South, South, West],
            &[0, 1, 4, 3, 2, 5],
        ),
        (
            &[North, East, South, South, South, West, West],
            &[0, 1, 2, 3, 4, 5, 6],
        ),
        (
            &[North, North, South, South, South, West, West, West, West],
            &[0, 1, 2, 3, 4, 5, 6, 7, 8],
        ),
    ];
    for &(sides, expected) in cases {
        let mut graph = graph(vec![node("A"), node("B")], vec![]);
        graph.layerless_nodes[NodeId::new(0)].port_constraints = PortConstraints::FixedSide;
        graph.set_node_layer(NodeId::new(0), 0);
        for (index, side) in sides.iter().copied().enumerate() {
            let outgoing = index % 2 == 0;
            let local = graph
                .add_port(
                    NodeId::new(0),
                    if outgoing {
                        PortType::Output
                    } else {
                        PortType::Input
                    },
                    side,
                    LPoint { x: 0.0, y: 0.0 },
                )
                .unwrap();
            let remote = graph
                .add_port(
                    NodeId::new(1),
                    if outgoing {
                        PortType::Input
                    } else {
                        PortType::Output
                    },
                    side.opposed(),
                    LPoint { x: 0.0, y: 0.0 },
                )
                .unwrap();
            graph.layerless_nodes[NodeId::new(0)].ports[local.port].id = index.to_string();
            let (source, target) = if outgoing {
                (local, remote)
            } else {
                (remote, local)
            };
            graph
                .add_edge(layered_edge(&format!("edge{index}"), source, target))
                .unwrap();
        }

        sort_port_lists(&mut graph);

        let actual = graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.id.parse::<usize>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "source range behavior for {sides:?}");
        for (position, &original) in expected.iter().enumerate() {
            let edge = &graph.edges[EdgeId::new(original)];
            let (local, remote) = if original % 2 == 0 {
                (edge.source(), edge.target())
            } else {
                (edge.target(), edge.source())
            };
            // The sort moved the port in the list. The edge still ends at the same port.
            assert_eq!(
                local,
                PortRef {
                    node: NodeId::new(0),
                    port: PortId::new(original)
                }
            );
            assert_eq!(
                graph.layerless_nodes[NodeId::new(0)]
                    .ports
                    .position(PortId::new(original)),
                Some(position)
            );
            assert_eq!(
                remote,
                PortRef {
                    node: NodeId::new(1),
                    port: PortId::new(original)
                }
            );
            let port = &graph.layerless_nodes[NodeId::new(0)].ports[PortId::new(original)];
            if original % 2 == 0 {
                assert_eq!(port.outgoing_edges(), vec![EdgeId::new(original)]);
                assert!(port.incoming_edges().is_empty());
            } else {
                assert_eq!(port.incoming_edges(), vec![EdgeId::new(original)]);
                assert!(port.outgoing_edges().is_empty());
            }
            assert!(graph.edge_source_attached(EdgeId::new(original)));
            assert!(graph.edge_target_attached(EdgeId::new(original)));
        }
    }
}

#[test]
fn west_range_is_recomputed_after_south_temporarily_crosses_sides() {
    let mut graph = graph(vec![node("A")], vec![]);
    for (index, side) in [
        PortSide::North,
        PortSide::East,
        PortSide::South,
        PortSide::South,
        PortSide::South,
        PortSide::West,
        PortSide::West,
    ]
    .into_iter()
    .enumerate()
    {
        let port = graph
            .add_port(
                NodeId::new(0),
                PortType::Input,
                side,
                LPoint { x: 0.0, y: 0.0 },
            )
            .unwrap();
        graph.layerless_nodes[NodeId::new(0)].ports[port.port].id = index.to_string();
    }
    reverse_side_range(&mut graph, NodeId::new(0), PortSide::South);
    let after_south = graph.layerless_nodes[NodeId::new(0)]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(after_south, vec!["0", "1", "5", "4", "3", "2", "6"]);

    reverse_side_range(&mut graph, NodeId::new(0), PortSide::West);
    let after_west = graph.layerless_nodes[NodeId::new(0)]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(after_west, vec!["0", "1", "2", "3", "4", "5", "6"]);
}

#[test]
fn fixed_order_ports_fall_back_to_position_without_both_indices() {
    let mut graph = graph(vec![node("A")], vec![]);
    let a = graph.node_named("A");
    graph.layerless_nodes[a].port_constraints = PortConstraints::FixedOrder;
    let left = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::North,
            LPoint { x: 1.0, y: 0.0 },
        )
        .unwrap();
    let right = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::North,
            LPoint { x: 9.0, y: 0.0 },
        )
        .unwrap();
    graph.layerless_nodes[a].ports[left.port].id = "left".to_string();
    graph.layerless_nodes[a].ports[right.port].id = "right".to_string();
    graph.layerless_nodes[a].ports[left.port].port_index = Some(99);
    graph.set_node_layer(a, 0);

    sort_port_lists(&mut graph);

    assert_eq!(
        graph.layerless_nodes[a]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "right"]
    );
}

#[test]
fn sort_by_input_model_orders_layers_by_model_order() {
    let mut graph = graph(
        vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
        vec![
            edge("Top-Right", "Top", "Right"),
            edge("Bottom-Left", "Bottom", "Left"),
        ],
    );
    layer_network_simplex(&mut graph).unwrap();
    process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
    sort_port_lists(&mut graph);

    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    let target_layer = graph.layerless_nodes[left].layer_index().unwrap();
    graph.set_layer_nodes(target_layer, vec![right, left]);

    sort_by_input_model(&mut graph);

    assert_eq!(graph.layers()[target_layer].nodes(), vec![left, right]);
}

#[test]
fn model_order_node_source_descriptor_uses_last_cross_layer_port() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    for index in 0..3 {
        graph
            .layerless_nodes
            .push(LNode::new(format!("node-{index}"), 10.0, 10.0, Some(index)));
    }
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(2), 1);

    let first_source = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let last_source = graph
        .add_port(
            NodeId::new(1),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let first_target = graph
        .add_port(
            NodeId::new(2),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let last_target = graph
        .add_port(
            NodeId::new(2),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    graph
        .add_edge(layered_edge("first", first_source, first_target))
        .unwrap();
    graph
        .add_edge(layered_edge("last", last_source, last_target))
        .unwrap();

    assert_eq!(
        last_previous_layer_source_port(&graph, NodeId::new(2)),
        Some(last_source)
    );
}

#[test]
fn sort_by_input_model_matches_source_outgoing_second_port_order() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
    );
    graph
        .layerless_nodes
        .push(LNode::new("A", 80.0, 40.0, Some(0)));
    graph
        .layerless_nodes
        .push(LNode::new("dummy-1", 0.0, 0.0, None));
    graph
        .layerless_nodes
        .push(LNode::new("dummy-2", 0.0, 0.0, None));
    graph.layerless_nodes[NodeId::new(1)].set_kind(LNodeKind::LongEdge);
    graph.layerless_nodes[NodeId::new(2)].set_kind(LNodeKind::LongEdge);
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 1);
    graph.set_node_layer(NodeId::new(2), 1);
    let first = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let second = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let first_target = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let second_target = graph
        .add_port(
            NodeId::new(2),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    graph
        .add_edge({
            let mut built_edge =
                crate::graph::LayeredEdge::new("A-dummy-1".to_string(), first, first_target);
            built_edge.source_node_id = "A".to_string();
            built_edge.target_node_id = "dummy-1".to_string();
            built_edge.model_order = Some(1);
            built_edge
        })
        .unwrap();
    graph
        .add_edge({
            let mut built_edge =
                crate::graph::LayeredEdge::new("A-dummy-2".to_string(), second, second_target);
            built_edge.source_node_id = "A".to_string();
            built_edge.target_node_id = "dummy-2".to_string();
            built_edge.model_order = Some(0);
            built_edge
        })
        .unwrap();

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["A:0", "A:1"]
    );

    sort_by_input_model(&mut graph);

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["A:0", "A:1"]
    );
}

#[test]
fn long_edge_target_preprocessing_uses_original_target_node() {
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

    let a = graph.node_named("A");
    let d = graph.node_named("D");
    let orders = long_edge_target_node_preprocessing(&mut graph, a);

    assert_eq!(orders.get(&d), Some(&3));
    assert!(
        graph.layerless_nodes[a]
            .ports
            .iter()
            .any(|port| port.long_edge_target_node == Some(d))
    );
}

#[test]
fn target_node_rejects_cyclic_long_edge_chain() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    for index in 0..3 {
        graph
            .layerless_nodes
            .push(LNode::new(format!("node-{index}"), 10.0, 10.0, Some(index)));
    }
    graph.layerless_nodes[NodeId::new(1)].set_kind(LNodeKind::LongEdge);
    graph.layerless_nodes[NodeId::new(2)].set_kind(LNodeKind::LongEdge);

    let source = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let first_input = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let first_output = graph
        .add_port(
            NodeId::new(1),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let cycle_input = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let second_input = graph
        .add_port(
            NodeId::new(2),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let second_output = graph
        .add_port(
            NodeId::new(2),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();

    graph
        .add_edge(layered_edge("source", source, first_input))
        .unwrap();
    graph
        .add_edge(layered_edge("forward", first_output, second_input))
        .unwrap();
    graph
        .add_edge(layered_edge("cycle", second_output, cycle_input))
        .unwrap();

    assert_eq!(target_node(&graph, source), None);
}

// The two ports share one id string, so only the `PortId` tells them apart.
//
// Mutation: leave the ports in the order the sweep put them, by skipping the
// `reorder_node_ports` call in `SweepCopy::transfer_node_and_port_orders_to_graph`.
#[test]
fn sweep_copy_restores_the_port_order_by_port_id_when_id_strings_repeat() {
    let mut graph = graph(vec![node("A")], vec![]);
    let a = graph.node_named("A");
    graph.layerless_nodes[a].ports.clear();
    let first = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::East,
            LPoint { x: 0.0, y: 0.0 },
        )
        .unwrap();
    let second = graph
        .add_port(
            a,
            PortType::Output,
            PortSide::East,
            LPoint { x: 0.0, y: 0.0 },
        )
        .unwrap();
    graph.layerless_nodes[a].ports[first.port].id = "shared".to_string();
    graph.layerless_nodes[a].ports[first.port].crossing_minimization_id = Some(7);
    graph.layerless_nodes[a].ports[second.port].id = "shared".to_string();
    graph.layerless_nodes[a].ports[second.port].crossing_minimization_id = Some(8);
    graph.set_node_layer(a, 0);

    let copy = SweepCopy::new(&graph, &[vec![a]]);
    graph.reorder_node_ports(a, crate::graph::port_ids([1, 0]));

    assert!(copy.transfer_node_and_port_orders_to_graph(&mut graph, false));
    assert_eq!(
        graph.layerless_nodes[a]
            .ports
            .iter()
            .map(|port| port.crossing_minimization_id)
            .collect::<Vec<_>>(),
        vec![Some(7), Some(8)]
    );
}

// --- Sides of ports whose dummies the minimization placed. ---

/// A node `a` in layer 1 with a north port and a south port that may switch sides, a
/// neighbour in layer 0 and the dummies the pre-processor makes. Returns the graph, the node
/// and the dummies of the north and of the south port.
fn node_with_movable_ports() -> (LGraph, NodeId, NodeId, NodeId) {
    let mut graph = LGraph::new(
        "g",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
    );
    let neighbour = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("n", 10.0, 10.0, None));
    graph.set_node_layer(neighbour, 0);
    for _ in 0..2 {
        graph.add_port(
            neighbour,
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        );
    }
    let a = graph.next_node_id();
    let mut node = LNode::new("a", 40.0, 20.0, None);
    node.port_constraints = PortConstraints::FixedSide;
    graph.layerless_nodes.push(node);
    graph.set_node_layer(a, 1);
    for (index, side) in [PortSide::North, PortSide::South].into_iter().enumerate() {
        graph.add_port(a, PortType::Input, side, LPoint::default());
        graph.layerless_nodes[a].ports[PortId::new(index)].allow_non_flow_ports_to_switch_sides =
            true;
        graph.add_edge(layered_edge(
            &format!("e{index}"),
            PortRef {
                node: neighbour,
                port: PortId::new(index),
            },
            PortRef {
                node: a,
                port: PortId::new(index),
            },
        ));
    }
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let dummy = |port: usize| {
        graph.layerless_nodes[a].ports[PortId::new(port)]
            .port_dummy
            .as_ref()
            .unwrap()
            .node
    };
    let (north, south) = (dummy(0), dummy(1));
    (graph, a, north, south)
}

fn side_of(graph: &LGraph, node: NodeId, port_id: usize) -> PortSide {
    graph.layerless_nodes[node].ports[PortId::new(port_id)].side
}

// Mutation: leave the sides of the ports alone.
#[test]
fn a_dummy_on_the_other_side_of_its_node_switches_the_side_of_its_port() {
    let (mut graph, a, north, south) = node_with_movable_ports();
    // The dummies trade places around the node: the south dummy first, the north one last.
    graph.set_layer_nodes(1, vec![south, a, north]);

    correct_north_south_port_sides(&mut graph);

    let sides = graph.layerless_nodes[a]
        .ports
        .iter()
        .map(|port| (port.id.clone(), port.side))
        .collect::<Vec<_>>();
    // The port that was north (port 0) is south now, and the other way round. The list is
    // sorted by side again: north before south.
    assert_eq!(side_of(&graph, a, 0), PortSide::South);
    assert_eq!(side_of(&graph, a, 1), PortSide::North);
    assert_eq!(sides[0].0, "a:1");
    assert_eq!(sides[1].0, "a:0");
}

// Mutation: leave the correction out of the transfer of the best sweep to the graph.
#[test]
fn the_transfer_of_a_sweep_corrects_the_sides_of_ports_that_switched() {
    let (mut graph, a, north, south) = node_with_movable_ports();
    initialize_crossing_minimization_port_ids(&mut graph);
    let mut order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    order[1] = vec![south, a, north];
    let sweep = SweepCopy::new(&graph, &order);

    assert!(sweep.transfer_node_and_port_orders_to_graph(&mut graph, true));

    assert_eq!(graph.layers()[1].nodes(), vec![south, a, north]);
    // The northern port is below its dummy and became the southern one: it is last now.
    assert_eq!(graph.layerless_nodes[a].ports.first().unwrap().id, "a:1");
    assert_eq!(side_of(&graph, a, 0), PortSide::South);
    assert_eq!(side_of(&graph, a, 1), PortSide::North);
    assert_eq!(
        graph.layerless_nodes[a].port_constraints,
        PortConstraints::FixedOrder
    );
}

// Mutation: switch the sides although the dummies are where the sides say.
#[test]
fn dummies_on_the_expected_side_leave_the_sides_of_the_ports_alone() {
    let (mut graph, a, north, south) = node_with_movable_ports();
    assert_eq!(graph.layers()[1].nodes(), vec![north, a, south]);

    correct_north_south_port_sides(&mut graph);

    assert_eq!(side_of(&graph, a, 0), PortSide::North);
    assert_eq!(side_of(&graph, a, 1), PortSide::South);
    assert_eq!(graph.layerless_nodes[a].ports[PortId::new(0)].id, "a:0");
}

// Mutation: switch the side of the south port only (or of the north port only).
#[test]
fn a_north_and_a_south_port_switch_independently() {
    let (mut graph, a, north, south) = node_with_movable_ports();
    // Both dummies above the node: the south port becomes a north port.
    graph.set_layer_nodes(1, vec![north, south, a]);

    correct_north_south_port_sides(&mut graph);

    assert_eq!(side_of(&graph, a, 0), PortSide::North);
    assert_eq!(side_of(&graph, a, 1), PortSide::North);

    let (mut graph, a, north, south) = node_with_movable_ports();
    // Both below: the north port becomes a south port.
    graph.set_layer_nodes(1, vec![a, north, south]);

    correct_north_south_port_sides(&mut graph);

    assert_eq!(side_of(&graph, a, 0), PortSide::South);
    assert_eq!(side_of(&graph, a, 1), PortSide::South);
}

// Mutation: leave the ports of the dummies in the order they were made.
#[test]
fn the_ports_of_a_dummy_are_sorted_by_side_again() {
    let mut graph = LGraph::new(
        "g",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
    );
    let neighbour = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("n", 10.0, 10.0, None));
    graph.set_node_layer(neighbour, 0);
    graph.add_port(
        neighbour,
        PortType::Output,
        PortSide::East,
        LPoint::default(),
    );
    graph.add_port(
        neighbour,
        PortType::Input,
        PortSide::West,
        LPoint::default(),
    );
    let a = graph.next_node_id();
    let mut node = LNode::new("a", 40.0, 20.0, None);
    node.port_constraints = PortConstraints::FixedSide;
    graph.layerless_nodes.push(node);
    graph.set_node_layer(a, 1);
    graph.add_port(a, PortType::Input, PortSide::North, LPoint::default());
    graph.add_edge(layered_edge(
        "in",
        PortRef {
            node: neighbour,
            port: PortId::new(0),
        },
        PortRef {
            node: a,
            port: PortId::new(0),
        },
    ));
    graph.add_edge(layered_edge(
        "out",
        PortRef {
            node: a,
            port: PortId::new(0),
        },
        PortRef {
            node: neighbour,
            port: PortId::new(1),
        },
    ));
    crate::intermediate::preprocess_north_south_ports(&mut graph);
    let dummy = graph.layerless_nodes[a].ports[PortId::new(0)]
        .port_dummy
        .as_ref()
        .unwrap()
        .node;
    // The dummy has a west port first and an east port second.
    let before = graph.layerless_nodes[dummy]
        .ports
        .iter()
        .map(|port| port.side)
        .collect::<Vec<_>>();
    assert_eq!(before, [PortSide::West, PortSide::East]);

    correct_north_south_port_sides(&mut graph);

    // East comes before west, as in `PortListSorter`. The edges stayed at their ports.
    let after = graph.layerless_nodes[dummy]
        .ports
        .iter()
        .map(|port| {
            (
                port.side,
                port.incoming_edges().len(),
                port.outgoing_edges().len(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(after, [(PortSide::East, 0, 1), (PortSide::West, 1, 0)]);
    // The edges name the same ports as before: the west port is port 0, the east port 1.
    assert_eq!(
        graph.edges[EdgeId::new(0)].target(),
        PortRef {
            node: dummy,
            port: PortId::new(0)
        }
    );
    assert_eq!(
        graph.edges[EdgeId::new(1)].source(),
        PortRef {
            node: dummy,
            port: PortId::new(1)
        }
    );
}
