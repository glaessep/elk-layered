use super::*;
use crate::graph::{EdgeId, EdgeLabelPlacement, LayeredEdge, PortType};
use crate::graph::{NodeId, PortId};
use crate::options::{Alignment, ElkPadding, LayeredOptions, NodeLabelPlacement, PortConstraints};

fn graph(direction: ElkDirection) -> LGraph {
    LGraph::new(
        "root",
        LayeredOptions {
            direction,
            ..LayeredOptions::default()
        },
    )
}

#[test]
fn node_label_placement_primitives_cover_every_axis_mapping() {
    use crate::options::NodeLabelLocation::*;
    use crate::options::{NodeLabelLocation, NodeLabelPlacement};
    let set = NodeLabelPlacement::from_location;
    let mx = |l: NodeLabelLocation| set(l).mirror_x().location();
    let my = |l: NodeLabelLocation| set(l).mirror_y().location();
    let tr = |l: NodeLabelLocation| set(l).transpose().location();

    for (placement, mirrored_x, mirrored_y, transposed) in [
        (Fixed, Fixed, Fixed, Fixed),
        (
            InsideTopLeft,
            InsideTopRight,
            InsideBottomLeft,
            InsideTopLeft,
        ),
        (
            InsideTopCenter,
            InsideTopCenter,
            InsideBottomCenter,
            InsideCenterLeft,
        ),
        (
            InsideTopRight,
            InsideTopLeft,
            InsideBottomRight,
            InsideBottomLeft,
        ),
        (
            InsideCenterLeft,
            InsideCenterRight,
            InsideCenterLeft,
            InsideTopCenter,
        ),
        (InsideCenter, InsideCenter, InsideCenter, InsideCenter),
        (
            InsideCenterRight,
            InsideCenterLeft,
            InsideCenterRight,
            InsideBottomCenter,
        ),
        (
            InsideBottomLeft,
            InsideBottomRight,
            InsideTopLeft,
            InsideTopRight,
        ),
        (
            InsideBottomCenter,
            InsideBottomCenter,
            InsideTopCenter,
            InsideCenterRight,
        ),
        (
            InsideBottomRight,
            InsideBottomLeft,
            InsideTopRight,
            InsideBottomRight,
        ),
        (
            OutsideTopLeft,
            OutsideTopRight,
            OutsideBottomLeft,
            OutsideLeftTop,
        ),
        (
            OutsideTopCenter,
            OutsideTopCenter,
            OutsideBottomCenter,
            OutsideLeftCenter,
        ),
        (
            OutsideTopRight,
            OutsideTopLeft,
            OutsideBottomRight,
            OutsideLeftBottom,
        ),
        (
            OutsideBottomLeft,
            OutsideBottomRight,
            OutsideTopLeft,
            OutsideRightTop,
        ),
        (
            OutsideBottomCenter,
            OutsideBottomCenter,
            OutsideTopCenter,
            OutsideRightCenter,
        ),
        (
            OutsideBottomRight,
            OutsideBottomLeft,
            OutsideTopRight,
            OutsideRightBottom,
        ),
        (
            OutsideLeftTop,
            OutsideRightTop,
            OutsideLeftBottom,
            OutsideTopLeft,
        ),
        (
            OutsideLeftCenter,
            OutsideRightCenter,
            OutsideLeftCenter,
            OutsideTopCenter,
        ),
        (
            OutsideLeftBottom,
            OutsideRightBottom,
            OutsideLeftTop,
            OutsideTopRight,
        ),
        (
            OutsideRightTop,
            OutsideLeftTop,
            OutsideRightBottom,
            OutsideBottomLeft,
        ),
        (
            OutsideRightCenter,
            OutsideLeftCenter,
            OutsideRightCenter,
            OutsideBottomCenter,
        ),
        (
            OutsideRightBottom,
            OutsideLeftBottom,
            OutsideRightTop,
            OutsideBottomRight,
        ),
    ] {
        assert_eq!(mx(placement), mirrored_x);
        assert_eq!(my(placement), mirrored_y);
        assert_eq!(tr(placement), transposed);

        assert_eq!(mx(mirrored_x), placement);
        assert_eq!(my(mirrored_y), placement);
        assert_eq!(tr(transposed), placement);
    }
}

#[test]
fn alignment_primitives_follow_source_axes_and_round_trip() {
    for (alignment, mirrored_x, mirrored_y, transposed) in [
        (
            Alignment::Automatic,
            Alignment::Automatic,
            Alignment::Automatic,
            Alignment::Automatic,
        ),
        (
            Alignment::Left,
            Alignment::Right,
            Alignment::Left,
            Alignment::Top,
        ),
        (
            Alignment::Right,
            Alignment::Left,
            Alignment::Right,
            Alignment::Bottom,
        ),
        (
            Alignment::Top,
            Alignment::Top,
            Alignment::Bottom,
            Alignment::Left,
        ),
        (
            Alignment::Bottom,
            Alignment::Bottom,
            Alignment::Top,
            Alignment::Right,
        ),
        (
            Alignment::Center,
            Alignment::Center,
            Alignment::Center,
            Alignment::Center,
        ),
    ] {
        assert_eq!(mirror_alignment_x(alignment), mirrored_x);
        assert_eq!(mirror_alignment_y(alignment), mirrored_y);
        assert_eq!(transpose_alignment(alignment), transposed);

        assert_eq!(mirror_alignment_x(mirrored_x), alignment);
        assert_eq!(mirror_alignment_y(mirrored_y), alignment);
        assert_eq!(transpose_alignment(transposed), alignment);
    }
}

fn directional_label_graph(
    direction: ElkDirection,
    direction_congruency: DirectionCongruency,
) -> LGraph {
    let mut graph = graph(direction);
    graph.options.direction_congruency = direction_congruency;
    graph.options.node_labels_padding = ElkPadding {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    };

    let mut top_left = LNode::new("top-left", 20.0, 10.0, None);
    top_left.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopLeft);
    top_left.node_alignment = Alignment::Left;
    top_left.padding = LPadding {
        top: 5.0,
        right: 6.0,
        bottom: 7.0,
        left: 8.0,
    };
    top_left.labels.push(LLabel {
        text: "top-left".to_string(),
        size: LSize {
            width: 4.0,
            height: 2.0,
        },
        position: LPoint { x: 1.0, y: 2.0 },
        placement: EdgeLabelPlacement::Center,
        inline: false,
        label_side: None,
        end_label_edge: None,
        original_label_edge: None,
        source_index: None,
        node_label_placement: None,
    });

    let mut bottom_right = LNode::new("bottom-right", 20.0, 10.0, None);
    bottom_right.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideBottomRight);
    bottom_right.node_alignment = Alignment::Bottom;
    bottom_right.labels.push(LLabel {
        text: "bottom-right".to_string(),
        size: LSize {
            width: 5.0,
            height: 3.0,
        },
        position: LPoint { x: 9.0, y: 5.0 },
        placement: EdgeLabelPlacement::Center,
        inline: false,
        label_side: None,
        end_label_edge: None,
        original_label_edge: None,
        source_index: None,
        node_label_placement: None,
    });

    graph.add_node_for_test(top_left);
    graph.add_node_for_test(bottom_right);
    graph
}

#[derive(Debug, Clone, Copy)]
struct DirectionalLabelExpectation {
    direction: ElkDirection,
    padding: ElkPadding,
    top_left_placement: NodeLabelPlacement,
    bottom_right_placement: NodeLabelPlacement,
    top_left_alignment: Alignment,
    bottom_right_alignment: Alignment,
    top_left_label_position: LPoint,
    top_left_label_size: LSize,
}

fn assert_directional_label_transform(
    direction_congruency: DirectionCongruency,
    expectation: DirectionalLabelExpectation,
) {
    let mut graph = directional_label_graph(expectation.direction, direction_congruency);
    let original = graph.clone();

    transform_graph_direction(&mut graph, GraphTransformMode::ToInternalLeftToRight);

    assert_eq!(graph.options.node_labels_padding, expectation.padding);
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)]
            .node_label_placement
            .location(),
        expectation.top_left_placement.location()
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(1)]
            .node_label_placement
            .location(),
        expectation.bottom_right_placement.location()
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].node_alignment,
        expectation.top_left_alignment
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(1)].node_alignment,
        expectation.bottom_right_alignment
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[0].position,
        expectation.top_left_label_position
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].labels[0].size,
        expectation.top_left_label_size
    );

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    assert_eq!(graph, original);
}

#[test]
fn reading_direction_transforms_label_semantics_and_round_trips() {
    for expectation in [
        DirectionalLabelExpectation {
            direction: ElkDirection::Right,
            padding: ElkPadding {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopLeft,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomRight,
            ),
            top_left_alignment: Alignment::Left,
            bottom_right_alignment: Alignment::Bottom,
            top_left_label_position: LPoint { x: 1.0, y: 2.0 },
            top_left_label_size: LSize {
                width: 4.0,
                height: 2.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Left,
            padding: ElkPadding {
                top: 1.0,
                right: 4.0,
                bottom: 3.0,
                left: 2.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopRight,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomLeft,
            ),
            top_left_alignment: Alignment::Right,
            bottom_right_alignment: Alignment::Bottom,
            top_left_label_position: LPoint { x: 15.0, y: 2.0 },
            top_left_label_size: LSize {
                width: 4.0,
                height: 2.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Down,
            padding: ElkPadding {
                top: 4.0,
                right: 3.0,
                bottom: 2.0,
                left: 1.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopLeft,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomRight,
            ),
            top_left_alignment: Alignment::Top,
            bottom_right_alignment: Alignment::Right,
            top_left_label_position: LPoint { x: 2.0, y: 1.0 },
            top_left_label_size: LSize {
                width: 2.0,
                height: 4.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Up,
            padding: ElkPadding {
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
                left: 1.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomLeft,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopRight,
            ),
            top_left_alignment: Alignment::Bottom,
            bottom_right_alignment: Alignment::Right,
            top_left_label_position: LPoint { x: 2.0, y: 15.0 },
            top_left_label_size: LSize {
                width: 2.0,
                height: 4.0,
            },
        },
    ] {
        assert_directional_label_transform(DirectionCongruency::ReadingDirection, expectation);
    }
}

#[test]
fn rotation_congruency_transforms_label_semantics_and_round_trips() {
    for expectation in [
        DirectionalLabelExpectation {
            direction: ElkDirection::Right,
            padding: ElkPadding {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopLeft,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomRight,
            ),
            top_left_alignment: Alignment::Left,
            bottom_right_alignment: Alignment::Bottom,
            top_left_label_position: LPoint { x: 1.0, y: 2.0 },
            top_left_label_size: LSize {
                width: 4.0,
                height: 2.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Left,
            padding: ElkPadding {
                top: 3.0,
                right: 4.0,
                bottom: 1.0,
                left: 2.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomRight,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopLeft,
            ),
            top_left_alignment: Alignment::Right,
            bottom_right_alignment: Alignment::Top,
            top_left_label_position: LPoint { x: 15.0, y: 6.0 },
            top_left_label_size: LSize {
                width: 4.0,
                height: 2.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Down,
            padding: ElkPadding {
                top: 4.0,
                right: 1.0,
                bottom: 2.0,
                left: 3.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopRight,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomLeft,
            ),
            top_left_alignment: Alignment::Top,
            bottom_right_alignment: Alignment::Left,
            top_left_label_position: LPoint { x: 6.0, y: 1.0 },
            top_left_label_size: LSize {
                width: 2.0,
                height: 4.0,
            },
        },
        DirectionalLabelExpectation {
            direction: ElkDirection::Up,
            padding: ElkPadding {
                top: 2.0,
                right: 3.0,
                bottom: 4.0,
                left: 1.0,
            },
            top_left_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideBottomLeft,
            ),
            bottom_right_placement: NodeLabelPlacement::from_location(
                crate::options::NodeLabelLocation::InsideTopRight,
            ),
            top_left_alignment: Alignment::Bottom,
            bottom_right_alignment: Alignment::Right,
            top_left_label_position: LPoint { x: 2.0, y: 15.0 },
            top_left_label_size: LSize {
                width: 2.0,
                height: 4.0,
            },
        },
    ] {
        assert_directional_label_transform(DirectionCongruency::Rotation, expectation);
    }
}

#[test]
fn reading_direction_down_transposes_graph() {
    let mut graph = graph(ElkDirection::Down);
    let mut node = LNode::new("A", 20.0, 10.0, None);
    node.position = LPoint { x: 3.0, y: 7.0 };
    let port = graph.add_node_for_test(node);
    graph.layerless_nodes[port].ports[PortId::new(0)].position = LPoint { x: 1.0, y: 2.0 };
    graph.layerless_nodes[port].ports[PortId::new(0)].anchor = LPoint { x: 3.0, y: 4.0 };
    graph.layerless_nodes[port].ports[PortId::new(0)].size = LSize {
        width: 5.0,
        height: 6.0,
    };
    graph.layerless_nodes[port].ports[PortId::new(0)].side = PortSide::North;
    graph.size = LSize {
        width: 100.0,
        height: 80.0,
    };
    graph.padding = LPadding {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    };

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    let node = &graph.layerless_nodes[port];
    assert_eq!(node.position, LPoint { x: 7.0, y: 3.0 });
    assert_eq!(
        node.size,
        LSize {
            width: 10.0,
            height: 20.0
        }
    );
    assert_eq!(
        node.ports[PortId::new(0)].position,
        LPoint { x: 2.0, y: 1.0 }
    );
    assert_eq!(node.ports[PortId::new(0)].anchor, LPoint { x: 4.0, y: 3.0 });
    assert_eq!(node.ports[PortId::new(0)].side, PortSide::West);
    assert_eq!(
        graph.size,
        LSize {
            width: 80.0,
            height: 100.0
        }
    );
    assert_eq!(
        graph.padding,
        LPadding {
            top: 4.0,
            right: 3.0,
            bottom: 2.0,
            left: 1.0
        }
    );
}

#[test]
fn rotation_down_to_input_direction_rotates_counter_clockwise() {
    let mut graph = graph(ElkDirection::Down);
    graph.options.direction_congruency = DirectionCongruency::Rotation;
    let mut node = LNode::new("A", 20.0, 10.0, None);
    node.position = LPoint { x: 3.0, y: 7.0 };
    let node = graph.add_node_for_test(node);

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    assert_eq!(
        graph.layerless_nodes[node].size,
        LSize {
            width: 10.0,
            height: 20.0
        }
    );
    assert_eq!(
        graph.layerless_nodes[node].position,
        LPoint { x: 7.0, y: 0.0 }
    );
}

#[test]
fn left_transform_mirrors_external_port_layer_constraint() {
    let mut graph = graph(ElkDirection::Left);
    let mut node = LNode::new("external", 0.0, 0.0, None);
    node.set_kind(crate::graph::LNodeKind::ExternalPort);
    node.layer_constraint = LayerConstraint::FirstSeparate;
    node.external_port_mut().unwrap().side = PortSide::West;
    let node = graph.add_node_for_test(node);

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    assert_eq!(
        graph.layerless_nodes[node].layer_constraint,
        LayerConstraint::LastSeparate
    );
    assert_eq!(
        graph.layerless_nodes[node].external_port().unwrap().side,
        PortSide::East
    );
}

#[test]
fn transpose_preserves_external_port_layer_constraint_through_in_layer_constraint() {
    let mut graph = graph(ElkDirection::Down);
    let mut node = LNode::new("external", 0.0, 0.0, None);
    node.set_kind(crate::graph::LNodeKind::ExternalPort);
    node.layer_constraint = LayerConstraint::FirstSeparate;
    node.external_port_mut().unwrap().side = PortSide::South;
    let node = graph.add_node_for_test(node);

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    assert_eq!(
        graph.layerless_nodes[node].layer_constraint,
        LayerConstraint::None
    );
    assert_eq!(
        graph.layerless_nodes[node]
            .external_port()
            .unwrap()
            .in_layer_constraint,
        InLayerConstraint::Top
    );
    assert_eq!(
        graph.layerless_nodes[node].external_port().unwrap().side,
        PortSide::East
    );

    transform_graph_direction(&mut graph, GraphTransformMode::ToInternalLeftToRight);

    assert_eq!(
        graph.layerless_nodes[node].layer_constraint,
        LayerConstraint::FirstSeparate
    );
    assert_eq!(
        graph.layerless_nodes[node]
            .external_port()
            .unwrap()
            .in_layer_constraint,
        InLayerConstraint::None
    );
    assert_eq!(
        graph.layerless_nodes[node].external_port().unwrap().side,
        PortSide::South
    );
}

// Mutation: leave `layer_constraint_explicit` unset where the transpose turns a top or bottom
// in-layer constraint into a separate layer constraint.
#[test]
fn transpose_marks_a_separate_layer_constraint_of_an_external_port_explicit() {
    for (in_layer, layer_constraint) in [
        (InLayerConstraint::Top, LayerConstraint::FirstSeparate),
        (InLayerConstraint::Bottom, LayerConstraint::LastSeparate),
    ] {
        let mut graph = graph(ElkDirection::Down);
        let mut node = LNode::new("external", 0.0, 0.0, None);
        node.set_kind(crate::graph::LNodeKind::ExternalPort);
        node.external_port_mut().unwrap().side = PortSide::North;
        node.external_port_mut().unwrap().in_layer_constraint = in_layer;
        let node = graph.add_node_for_test(node);
        assert!(!graph.is_layer_constraint_explicit(node));

        transform_graph_direction(&mut graph, GraphTransformMode::ToInternalLeftToRight);

        assert_eq!(
            graph.layerless_nodes[node].layer_constraint,
            layer_constraint
        );
        assert!(graph.is_layer_constraint_explicit(node), "{in_layer:?}");
    }
}

// Mutation: leave `layer_constraint_explicit` unset where the transpose turns a separate layer
// constraint into an in-layer constraint.
#[test]
fn transpose_marks_the_layer_constraint_explicit_where_it_clears_a_separate_one() {
    for layer_constraint in [
        LayerConstraint::FirstSeparate,
        LayerConstraint::LastSeparate,
    ] {
        let mut graph = graph(ElkDirection::Down);
        let mut node = LNode::new("external", 0.0, 0.0, None);
        node.set_kind(crate::graph::LNodeKind::ExternalPort);
        node.layer_constraint = layer_constraint;
        node.external_port_mut().unwrap().side = PortSide::East;
        let node = graph.add_node_for_test(node);
        assert!(!graph.is_layer_constraint_explicit(node));

        transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

        assert_eq!(
            graph.layerless_nodes[node].layer_constraint,
            LayerConstraint::None
        );
        assert!(
            graph.is_layer_constraint_explicit(node),
            "{layer_constraint:?}"
        );
    }
}

// Mutation: leave `layer_constraint_explicit` unset where the transpose gives a separate layer
// constraint. The directions that mirror around the transpose (`Up`, and rotation) must keep
// the flag.
#[test]
fn every_direction_keeps_the_explicit_flag_of_a_transposed_external_port() {
    use DirectionCongruency::{ReadingDirection, Rotation};
    for (direction, congruency, top, bottom) in [
        (
            ElkDirection::Down,
            ReadingDirection,
            LayerConstraint::FirstSeparate,
            LayerConstraint::LastSeparate,
        ),
        (
            ElkDirection::Up,
            ReadingDirection,
            LayerConstraint::FirstSeparate,
            LayerConstraint::LastSeparate,
        ),
        (
            ElkDirection::Down,
            Rotation,
            LayerConstraint::LastSeparate,
            LayerConstraint::FirstSeparate,
        ),
        (
            ElkDirection::Up,
            Rotation,
            LayerConstraint::FirstSeparate,
            LayerConstraint::LastSeparate,
        ),
    ] {
        for (in_layer, want) in [
            (InLayerConstraint::Top, top),
            (InLayerConstraint::Bottom, bottom),
        ] {
            let mut graph = graph(direction);
            graph.options.direction_congruency = congruency;
            let mut node = LNode::new("external", 0.0, 0.0, None);
            node.set_kind(crate::graph::LNodeKind::ExternalPort);
            node.external_port_mut().unwrap().side = PortSide::North;
            node.external_port_mut().unwrap().in_layer_constraint = in_layer;
            let node = graph.add_node_for_test(node);

            transform_graph_direction(&mut graph, GraphTransformMode::ToInternalLeftToRight);

            let label = format!("{direction:?} {congruency:?} {in_layer:?}");
            assert_eq!(
                graph.layerless_nodes[node].layer_constraint, want,
                "{label}"
            );
            assert!(graph.is_layer_constraint_explicit(node), "{label}");
        }
    }
}

#[test]
fn edge_bend_points_and_labels_are_transformed() {
    let mut graph = graph(ElkDirection::Down);
    let source = graph.add_node_for_test(LNode::new("A", 10.0, 10.0, None));
    let target = graph.add_node_for_test(LNode::new("B", 10.0, 10.0, None));
    let source_port = graph.add_port(
        source,
        PortType::Output,
        PortSide::East,
        LPoint { x: 10.0, y: 5.0 },
    );
    let target_port = graph.add_port(
        target,
        PortType::Input,
        PortSide::West,
        LPoint { x: 0.0, y: 5.0 },
    );
    let edge = {
        let mut built_edge = LayeredEdge::new(
            "A-B".to_string(),
            source_port.unwrap(),
            target_port.unwrap(),
        );
        built_edge.source_node_id = "A".to_string();
        built_edge.target_node_id = "B".to_string();
        built_edge.labels = vec![LLabel {
            text: "label".to_string(),
            size: LSize {
                width: 12.0,
                height: 4.0,
            },
            position: LPoint { x: 30.0, y: 40.0 },
            placement: EdgeLabelPlacement::Center,
            inline: false,
            label_side: None,
            end_label_edge: None,
            original_label_edge: None,
            source_index: None,
            node_label_placement: None,
        }];
        built_edge.bend_points = vec![LPoint { x: 1.0, y: 2.0 }];
        built_edge.junction_points = vec![LPoint { x: 3.0, y: 4.0 }];
        built_edge
    };
    graph.add_edge(edge);

    transform_graph_direction(&mut graph, GraphTransformMode::ToInputDirection);

    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points[0],
        LPoint { x: 2.0, y: 1.0 }
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)].labels[0].size,
        LSize {
            width: 4.0,
            height: 12.0
        }
    );
    assert_eq!(
        graph.edges[EdgeId::new(0)].labels[0].position,
        LPoint { x: 40.0, y: 30.0 }
    );
}

// Mutation: leave the junction points out of one of `mirror_all_x`, `mirror_all_y` or
// `transpose_all`. The junction point then stays where the bend point has moved from.
#[test]
fn junction_points_move_like_bend_points_in_every_transformation() {
    for direction in [ElkDirection::Left, ElkDirection::Down, ElkDirection::Up] {
        for congruency in [
            DirectionCongruency::ReadingDirection,
            DirectionCongruency::Rotation,
        ] {
            for mode in [
                GraphTransformMode::ToInternalLeftToRight,
                GraphTransformMode::ToInputDirection,
            ] {
                let mut graph = graph(direction);
                graph.options.direction_congruency = congruency;
                graph.size = LSize {
                    width: 100.0,
                    height: 60.0,
                };
                let source = graph.add_node_for_test(LNode::new("A", 10.0, 10.0, None));
                let target = graph.add_node_for_test(LNode::new("B", 10.0, 10.0, None));
                let from = graph.add_port(
                    source,
                    PortType::Output,
                    PortSide::East,
                    LPoint { x: 10.0, y: 5.0 },
                );
                let to = graph.add_port(
                    target,
                    PortType::Input,
                    PortSide::West,
                    LPoint { x: 0.0, y: 5.0 },
                );
                let point = LPoint { x: 30.0, y: 20.0 };
                graph.add_edge({
                    let mut built_edge =
                        LayeredEdge::new("e".to_string(), from.unwrap(), to.unwrap());
                    built_edge.source_node_id = "A".to_string();
                    built_edge.target_node_id = "B".to_string();
                    built_edge.bend_points = vec![point];
                    built_edge.junction_points = vec![point];
                    built_edge
                });

                transform_graph_direction(&mut graph, mode);

                let edge = &graph.edges[EdgeId::new(0)];
                assert_eq!(
                    edge.junction_points, edge.bend_points,
                    "{direction:?} {congruency:?} {mode:?}"
                );
                assert_ne!(edge.bend_points[0], point, "{direction:?} {congruency:?}");
            }
        }
    }
}

trait TestGraphExt {
    fn add_node_for_test(&mut self, node: LNode) -> NodeId;
}

impl TestGraphExt for LGraph {
    fn add_node_for_test(&mut self, mut node: LNode) -> NodeId {
        let index = self.next_node_id();
        node.port_constraints = PortConstraints::FixedSide;
        node.ports.push(LPort::new(
            format!("{}:0", node.id),
            index,
            PortType::Output,
        ));
        self.layerless_nodes.push(node);
        index
    }
}
