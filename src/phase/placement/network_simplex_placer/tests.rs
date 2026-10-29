use super::*;
use crate::graph::LMargin;
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::{ElkDirection, LayeredOptions};

// Mutation: round with `(value + 0.5).floor()` again.
#[test]
fn a_port_position_rounds_as_elkjs_rounds() {
    assert_eq!(java_round(2.5), Ok(3.0));
    assert_eq!(java_round(-2.5), Ok(-2.0));
    assert_eq!(java_round(0.0), Ok(0.0));
    assert_eq!(java_round(0.499_999_999_999_999_94), Ok(0.0));
    assert_eq!(
        java_round(f64::NAN),
        Err(NetworkSimplexError::CoordinateOutOfRange)
    );
    assert_eq!(
        java_round(3e9),
        Err(NetworkSimplexError::CoordinateOutOfRange)
    );
}

fn node(id: &str, width: f64, height: f64) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width,
        height,
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
fn network_simplex_placer_respects_layer_order_and_spacing() {
    let mut graph = graph(
        vec![
            node("A", 80.0, 30.0),
            node("B", 80.0, 20.0),
            node("C", 80.0, 40.0),
        ],
        vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
    );
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(2), 1);
    graph.layerless_nodes[NodeId::new(0)].margin = LMargin {
        top: 2.0,
        bottom: 3.0,
        ..LMargin::default()
    };
    graph.layerless_nodes[NodeId::new(1)].margin = LMargin {
        top: 5.0,
        bottom: 7.0,
        ..LMargin::default()
    };

    place_nodes_network_simplex(&mut graph).unwrap();

    let required_gap = graph.layerless_nodes[NodeId::new(0)].size.height
        + graph.layerless_nodes[NodeId::new(0)].margin.bottom
        + vertical_spacing(&graph, NodeId::new(0), NodeId::new(1))
        + graph.layerless_nodes[NodeId::new(1)].margin.top;
    assert!(
        graph.layerless_nodes[NodeId::new(1)].position.y
            - graph.layerless_nodes[NodeId::new(0)].position.y
            >= required_gap
    );
    assert!(
        graph
            .layerless_nodes
            .iter()
            .all(|node| node.position.y.is_finite())
    );
}

#[test]
fn network_simplex_placer_handles_empty_layers() {
    let mut graph = graph(vec![node("A", 80.0, 30.0)], vec![]);
    graph.set_node_layer(NodeId::new(0), 1);

    place_nodes_network_simplex(&mut graph).unwrap();

    assert!(graph.layerless_nodes[NodeId::new(0)].position.y.is_finite());
}

// Mutation: in `is_flexible`, take the margins from the default instead of the node's own
// `ports_surrounding`.
#[test]
fn a_node_whose_own_ports_surrounding_leave_no_room_is_not_flexible() {
    let mut graph = graph(
        vec![
            node("A", 40.0, 20.0),
            node("B", 40.0, 20.0),
            node("X", 80.0, 160.0),
        ],
        vec![edge("A-X", "A", "X"), edge("B-X", "B", "X")],
    );
    let x = NodeId::new(2);
    graph.tables.placement_options.row_mut(x).flexibility = NodeFlexibility::PortPosition;

    graph.tables.placement_options.row_mut(x).ports_surrounding =
        Some(crate::options::SpacingMargin {
            top: 5.0,
            right: 0.0,
            bottom: 5.0,
            left: 0.0,
        });
    assert!(is_flexible(&graph, x));

    graph.tables.placement_options.row_mut(x).ports_surrounding =
        Some(crate::options::SpacingMargin {
            top: 100.0,
            right: 0.0,
            bottom: 100.0,
            left: 0.0,
        });
    assert!(!is_flexible(&graph, x));
}

#[test]
fn fixed_or_crowded_ports_preserve_their_precomputed_positions() {
    let mut graph = graph(
        vec![
            node("A", 40.0, 20.0),
            node("B", 40.0, 20.0),
            node("X", 80.0, 160.0),
        ],
        vec![edge("A-X", "A", "X"), edge("B-X", "B", "X")],
    );
    graph
        .tables
        .placement_options
        .row_mut(NodeId::new(2))
        .flexibility = NodeFlexibility::PortPosition;
    assert!(is_flexible(&graph, NodeId::new(2)));
    graph.layerless_nodes[NodeId::new(2)].port_constraints =
        crate::options::PortConstraints::FixedPos;
    assert!(!is_flexible(&graph, NodeId::new(2)));
    graph.layerless_nodes[NodeId::new(2)].port_constraints = crate::options::PortConstraints::Free;
    graph.layerless_nodes[NodeId::new(2)].size.height = 10.0;
    assert!(!is_flexible(&graph, NodeId::new(2)));
}

#[test]
fn flexible_large_coordinates_and_bottom_labels_remain_source_aligned() {
    use NodeLabelLocation::*;
    for height in [160.5, 100_000_000.5] {
        for placement in [
            InsideBottomLeft,
            InsideBottomCenter,
            InsideBottomRight,
            OutsideBottomLeft,
            OutsideBottomCenter,
            OutsideBottomRight,
            OutsideLeftBottom,
            OutsideRightBottom,
        ] {
            let mut middle = node("X", 80.0, height);
            middle.node_flexibility = NodeFlexibility::PortPosition;
            middle.node_label_placement =
                crate::options::NodeLabelPlacement::from_location(placement);
            let mut graph = graph(
                vec![node("A", 40.0, 20.0), middle, node("B", 40.0, 20.0)],
                vec![edge("A-X", "A", "X"), edge("X-B", "X", "B")],
            );
            for node in 0..3 {
                graph.set_node_layer(NodeId::new(node), node);
            }
            graph.options.direction = ElkDirection::Right;
            crate::intermediate::process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
            let mut label = crate::graph::LLabel::new("bottom", 20.0, 10.0);
            label.position.y = height + 7.0;
            graph.layerless_nodes[NodeId::new(1)].labels.push(label);

            place_nodes_network_simplex(&mut graph).unwrap();

            let middle = &graph.layerless_nodes[NodeId::new(1)];
            assert_eq!(middle.size.height, height);
            assert_eq!(middle.labels[0].position.y, height + 7.5, "{placement:?}");
            assert!(
                middle
                    .ports
                    .iter()
                    .all(|port| port.position.y >= 0.0 && port.position.y <= height.ceil())
            );
        }
    }
}

#[test]
fn flexible_height_outside_integer_range_propagates_from_pipeline() {
    let mut middle = node("X", 80.0, i32::MAX as f64 + 1.0);
    middle.node_flexibility = NodeFlexibility::PortPosition;
    let mut graph = graph(
        vec![node("A", 40.0, 20.0), middle, node("B", 40.0, 20.0)],
        vec![edge("A-X", "A", "X"), edge("X-B", "X", "B")],
    );
    graph.options.direction = ElkDirection::Right;
    graph.options.node_placement_strategy = crate::options::NodePlacementStrategy::NetworkSimplex;

    assert_eq!(
        crate::pipeline::execute_ported_processors(&mut graph),
        Err(crate::pipeline::PipelineError::NetworkSimplex(
            NetworkSimplexError::CoordinateOutOfRange
        )),
    );
}

#[test]
fn port_flexibility_matches_elkjs_0_9_3_geometry() {
    for flexibility in [NodeFlexibility::None, NodeFlexibility::PortPosition] {
        let mut middle = node("X", 80.0, 160.0);
        middle.node_flexibility = flexibility;
        middle.ports_surrounding = Some(crate::options::SpacingMargin {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        });
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".into(),
            options: LayeredOptions {
                direction: ElkDirection::Right,
                node_placement_strategy: crate::options::NodePlacementStrategy::NetworkSimplex,
                ..Default::default()
            },
            nodes: vec![
                node("A", 40.0, 20.0),
                node("B", 40.0, 50.0),
                middle,
                node("C", 40.0, 30.0),
                node("D", 40.0, 70.0),
            ],
            edges: vec![
                edge("A-X", "A", "X"),
                edge("B-X", "B", "X"),
                edge("X-C", "X", "C"),
                edge("X-D", "X", "D"),
            ],
        })
        .unwrap();
        crate::pipeline::execute_ported_processors(&mut graph).unwrap();
        // Published elkjs coordinates include root padding (12, 12).
        let expected_y = match flexibility {
            NodeFlexibility::None => [110.0, 40.0, 12.0, 50.0, 100.0],
            NodeFlexibility::PortPosition => [82.0, 12.0, 37.0, 22.0, 72.0],
        };
        for (node, y) in graph.layerless_nodes.iter().zip(expected_y) {
            assert_eq!(
                node.position.y + graph.padding.top,
                y,
                "{flexibility:?}: {}",
                node.id
            );
        }
        assert_eq!(graph.layerless_nodes[NodeId::new(2)].size.height, 160.0);
        let straight_edges = graph
            .edges
            .iter()
            .filter(|edge| {
                let source = &graph.layerless_nodes[edge.source().node];
                let target = &graph.layerless_nodes[edge.target().node];
                source.position.y + source.ports[edge.source().port].position.y
                    == target.position.y + target.ports[edge.target().port].position.y
            })
            .count();
        assert_eq!(
            straight_edges,
            if flexibility == NodeFlexibility::PortPosition {
                4
            } else {
                2
            }
        );
    }
}
