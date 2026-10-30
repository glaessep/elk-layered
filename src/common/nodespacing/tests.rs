use super::*;
use crate::graph::PortSide;
use crate::graph::{EdgeLabelPlacement, LLabel, LNode, LPoint, LSize, PortType};
use crate::options::{ElkDirection, LayeredOptions, NodeLabelPlacement, PortConstraints};

fn node_label(width: f64, height: f64) -> LLabel {
    LLabel {
        text: "label".to_string(),
        size: LSize { width, height },
        position: LPoint::default(),
        placement: EdgeLabelPlacement::Center,
        inline: false,
        label_side: None,
        end_label_edge: None,
        original_label_edge: None,
        source_index: None,
        node_label_placement: None,
    }
}

#[test]
fn label_and_node_size_calculation_places_inside_top_center_node_labels() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let mut node = LNode::new("group", 100.3125, 105.0, None);
    node.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopCenter);
    node.labels.push(LLabel {
        text: "three".to_string(),
        size: LSize {
            width: 38.765625,
            height: 22.0,
        },
        // LEFT direction preprocessing mirrors an unplaced label to this position.
        position: LPoint {
            x: -38.765625,
            y: 0.0,
        },
        placement: EdgeLabelPlacement::Center,
        inline: false,
        label_side: None,
        end_label_edge: None,
        original_label_edge: None,
        source_index: None,
        node_label_placement: None,
    });
    graph.layerless_nodes.push(node);
    graph.set_node_layer(NodeId::new(0), 0);

    calculate_label_and_node_sizes(&mut graph, [NodeId::new(0)]);
    calculate_node_margins(&mut graph, [NodeId::new(0)]);

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[0].position,
        LPoint {
            x: 30.7734375,
            y: 5.0,
        }
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].margin,
        LMargin::default()
    );
}

#[test]
fn horizontal_layout_mode_stacks_multiple_node_labels_vertically() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.direction = ElkDirection::Right;
    graph.options.spacing.label_label = 3.0;
    let mut node = LNode::new("group", 100.0, 80.0, None);
    node.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopCenter);
    node.labels.push(node_label(20.0, 10.0));
    node.labels.push(node_label(30.0, 12.0));
    graph.layerless_nodes.push(node);

    calculate_label_and_node_sizes(&mut graph, [NodeId::new(0)]);

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[0].position,
        LPoint { x: 40.0, y: 5.0 }
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[1].position,
        LPoint { x: 35.0, y: 18.0 }
    );
}

#[test]
fn vertical_layout_mode_stacks_transposed_node_labels_horizontally() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.options.direction = ElkDirection::Down;
    graph.options.spacing.label_label = 3.0;
    let mut node = LNode::new("group", 80.0, 100.0, None);
    node.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideCenterLeft);
    node.labels.push(node_label(10.0, 20.0));
    node.labels.push(node_label(12.0, 30.0));
    graph.layerless_nodes.push(node);

    calculate_label_and_node_sizes(&mut graph, [NodeId::new(0)]);

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[0].position,
        LPoint { x: 5.0, y: 40.0 }
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[1].position,
        LPoint { x: 18.0, y: 35.0 }
    );
}

// Mutation: leave the southern ports at the height they are placed at, which is the top of
// the node, or offset the northern ones too.
#[test]
fn southern_ports_sit_on_the_southern_border_for_every_port_constraint() {
    for constraints in [
        PortConstraints::FixedSide,
        PortConstraints::FixedRatio,
        PortConstraints::FixedPos,
    ] {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("A", 80.0, 40.0, None));
        let node = NodeId::new(0);
        graph.set_node_layer(node, 0);
        graph.layerless_nodes[node].port_constraints = constraints;
        let mut refs = Vec::new();
        for side in [PortSide::North, PortSide::South] {
            let port = graph
                .add_port(
                    node,
                    PortType::Output,
                    side,
                    crate::graph::LPoint::default(),
                )
                .unwrap();
            let data = &mut graph.layerless_nodes[node].ports[port.port];
            data.size = LSize {
                width: 6.0,
                height: 4.0,
            };
            data.border_offset = Some(1.5);
            refs.push(port.port);
        }

        calculate_label_and_node_sizes(&mut graph, [node]);

        let ports = &graph.layerless_nodes[node].ports;
        // The northern port sits outside above the node, the southern port below it.
        assert_eq!(ports[refs[0]].position.y, -4.0 - 1.5, "{constraints:?}");
        assert_eq!(ports[refs[1]].position.y, 40.0 + 1.5, "{constraints:?}");
    }
}

#[test]
fn label_and_node_size_calculation_places_free_ports_on_node_border() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("A", 80.0, 40.0, None));
    let node = NodeId::new(0);
    graph.set_node_layer(node, 0);
    let port = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::East,
            crate::graph::LPoint::default(),
        )
        .unwrap();
    graph.layerless_nodes[node].ports[port.port].set_side(PortSide::East);

    calculate_label_and_node_sizes(&mut graph, [node]);

    let port = &graph.layerless_nodes[node].ports[port.port];
    assert_eq!(port.position.x, graph.layerless_nodes[node].size.width);
    assert!(port.position.y > 0.0);
    assert!(port.position.y < graph.layerless_nodes[node].size.height);
}

// Mutation: place the southern ports at the top of the node, as they were before the node had
// a height.
#[test]
fn label_and_node_size_calculation_crams_distributed_ports_when_overhang_disabled() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("C", 69.578125, 54.0, None));
    let node = NodeId::new(0);
    graph.set_node_layer(node, 0);

    for _ in 0..5 {
        let port = graph
            .add_port(
                node,
                PortType::Output,
                PortSide::South,
                crate::graph::LPoint::default(),
            )
            .unwrap();
        graph.layerless_nodes[node].ports[port.port].set_side(PortSide::South);
    }

    calculate_label_and_node_sizes(&mut graph, [node]);

    let expected_spacing = graph.layerless_nodes[node].size.width / 6.0;
    for (index, port) in graph.layerless_nodes[node].ports.iter_with_ids() {
        let expected_x = expected_spacing * (5 - index.index()) as f64;
        assert!(
            (port.position.x - expected_x).abs() < 1e-9,
            "port {} x: expected {expected_x}, got {}",
            index.index(),
            port.position.x
        );
        assert_eq!(port.position.y, 54.0);
    }
}

// Mutation: in `place_free_ports`, use the graph's default port alignment for every node.
#[test]
fn a_node_places_its_free_ports_by_its_own_port_alignment() {
    let first_port_x = |alignment: Option<crate::options::PortAlignment>| {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("A", 200.0, 40.0, None));
        let node = NodeId::new(0);
        graph.set_node_layer(node, 0);
        if let Some(alignment) = alignment {
            *graph.tables.port_alignment.row_mut(node) = alignment;
        }
        let ports = [0, 1].map(|_| {
            let port = graph
                .add_port(
                    node,
                    PortType::Output,
                    PortSide::North,
                    crate::graph::LPoint::default(),
                )
                .unwrap();
            graph.layerless_nodes[node].ports[port.port].set_side(PortSide::North);
            graph.layerless_nodes[node].ports[port.port].size.width = 10.0;
            port
        });

        calculate_label_and_node_sizes(&mut graph, [node]);

        graph.layerless_nodes[node].ports[ports[0].port].position.x
    };

    let begin = first_port_x(Some(crate::options::PortAlignment::Begin));
    let end = first_port_x(Some(crate::options::PortAlignment::End));
    let default = first_port_x(None);

    assert!(begin < end, "begin {begin}, end {end}");
    assert_eq!(
        default,
        first_port_x(Some(LayeredOptions::default().port_alignment_default))
    );
}

#[test]
fn label_and_node_size_calculation_places_west_ports_top_to_bottom() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("A", 80.0, 70.0, None));
    let node = NodeId::new(0);
    graph.set_node_layer(node, 0);

    for id in ["bottom", "middle", "top"] {
        let port = graph
            .add_port(
                node,
                PortType::Input,
                PortSide::West,
                crate::graph::LPoint::default(),
            )
            .unwrap();
        graph.layerless_nodes[node].ports[port.port].id = id.to_string();
        graph.layerless_nodes[node].ports[port.port].set_side(PortSide::West);
    }

    calculate_label_and_node_sizes(&mut graph, [node]);

    let y_positions = graph.layerless_nodes[node]
        .ports
        .iter()
        .map(|port| (port.id.as_str(), port.position.y))
        .collect::<Vec<_>>();
    assert!(y_positions[2].1 < y_positions[1].1);
    assert!(y_positions[1].1 < y_positions[0].1);
}

#[test]
fn node_margin_calculation_includes_ports_and_port_labels() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let node = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("A", 80.0, 40.0, None));
    graph.set_node_layer(node, 0);
    let port = graph
        .add_port(
            node,
            PortType::Output,
            PortSide::East,
            crate::graph::LPoint { x: 80.0, y: 10.0 },
        )
        .unwrap();
    graph.layerless_nodes[node].ports[port.port].size = LSize {
        width: 8.0,
        height: 8.0,
    };
    graph.layerless_nodes[node].ports[port.port]
        .labels
        .push(crate::graph::LLabel {
            text: "p".to_string(),
            size: LSize {
                width: 12.0,
                height: 6.0,
            },
            position: crate::graph::LPoint { x: 10.0, y: 0.0 },
            placement: crate::graph::EdgeLabelPlacement::Center,
            inline: false,
            label_side: None,
            end_label_edge: None,
            original_label_edge: None,
            source_index: None,
            node_label_placement: None,
        });

    calculate_node_margins(&mut graph, [node]);

    assert_eq!(graph.layerless_nodes[node].margin.left, 0.0);
    assert!(graph.layerless_nodes[node].margin.right >= 22.0);
}
