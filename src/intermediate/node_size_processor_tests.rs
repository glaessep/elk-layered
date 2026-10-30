//! Tests of `InLayerConstraintProcessor`, `LabelAndNodeSizeProcessor` and
//! `InnermostNodeMarginCalculator`.

use crate::graph::{EdgeLabelPlacement, LLabel, LSize, PortSide};
use crate::graph::{InLayerConstraint, LGraph, LNodeKind, NodeId};
use crate::graph::{OtherGraphs, PortId};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::intermediate::{
    calculate_innermost_node_margins, calculate_label_and_node_sizes, process_in_layer_constraints,
};
use crate::options::{ElkDirection, LayeredOptions};

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
        options: LayeredOptions {
            direction: ElkDirection::Right,
            ..LayeredOptions::default()
        },
        nodes,
        edges,
    })
    .unwrap()
}

#[test]
fn label_dummy_represented_edge_labels_do_not_expand_node_margin() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
    );
    let mut dummy = crate::graph::LNode::new("label", 13.0, 24.0, None);
    dummy.set_kind(LNodeKind::Label);
    dummy
        .label_dummy_mut()
        .unwrap()
        .represented_labels
        .push(LLabel {
            text: "edge label".to_string(),
            size: LSize {
                width: 13.0,
                height: 24.0,
            },
            position: crate::graph::LPoint { x: 63.0, y: 0.0 },
            placement: EdgeLabelPlacement::Center,
            inline: true,
            label_side: None,
            end_label_edge: None,
            original_label_edge: None,
            source_index: None,
            node_label_placement: None,
        });
    graph.layerless_nodes.push(dummy);
    graph.set_node_layer(NodeId::new(0), 0);

    calculate_innermost_node_margins(&mut graph);

    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)].margin,
        Default::default()
    );
}

// Mutation: in `process_in_layer_constraints`, put a node with the `Top` constraint in the
// middle group.
#[test]
fn in_layer_constraint_processor_moves_top_and_bottom_nodes() {
    let mut graph = graph(vec![node("A"), node("B"), node("C"), node("D")], vec![]);
    for index in graph.node_ids() {
        graph.set_node_layer(index, 0);
    }
    for index in [1, 2] {
        graph.layerless_nodes[NodeId::new(index)].set_kind(LNodeKind::ExternalPort);
    }
    graph.layerless_nodes[NodeId::new(2)]
        .external_port_mut()
        .unwrap()
        .in_layer_constraint = InLayerConstraint::Top;
    graph.layerless_nodes[NodeId::new(1)]
        .external_port_mut()
        .unwrap()
        .in_layer_constraint = InLayerConstraint::Bottom;

    process_in_layer_constraints(&mut graph);

    let order = graph.layers()[0]
        .nodes()
        .iter()
        .map(|node| graph.layerless_nodes[*node].id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["C", "A", "D", "B"]);
}

#[test]
fn label_and_node_size_processor_delegates_common_node_spacing() {
    let mut graph = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    graph.set_node_layer(a, 0);
    graph.set_node_layer(b, 1);
    graph.layerless_nodes[a].ports[PortId::new(0)].set_side(PortSide::East);

    calculate_label_and_node_sizes(&mut graph, OtherGraphs::none());

    let port = &graph.layerless_nodes[a].ports[PortId::new(0)];
    assert_eq!(port.position.x, graph.layerless_nodes[a].size.width);
    assert!(port.position.y > 0.0);
    assert!(port.position.y < graph.layerless_nodes[a].size.height);
}

// --- Labels of external port dummies: `LabelAndNodeSizeProcessor.placeExternalPortDummyLabels`. ---

mod external_port_dummy_labels {
    use super::*;
    use crate::graph::{
        DummyLabelSpacings, ExternalPortData, LNode, LPoint, LayeredEdge, PortRef, PortType,
        create_external_port_dummy,
    };
    use crate::intermediate::label_and_node_size_processor::place_external_port_dummy_labels;
    use crate::options::{PortConstraints, PortLabelPlacement};

    /// The dummy of a port of 6 by 4 on `side`, with a label of 20 by 10 and one of 30 by 6, in
    /// a graph with the layered node `other` beside it. Gives the graph and the dummy.
    fn dummy_with_labels(side: PortSide, spacings: DummyLabelSpacings) -> (LGraph, NodeId) {
        let mut graph = LGraph::new(
            "root",
            LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        );
        let dummy = graph.push_node_with(|node| {
            let mut dummy = create_external_port_dummy(
                node,
                "dummy",
                "p",
                PortType::Output,
                PortConstraints::FixedSide,
                side,
                0,
                LPoint::default(),
                LSize {
                    width: 6.0,
                    height: 4.0,
                },
                LSize {
                    width: 80.0,
                    height: 40.0,
                },
                0.0,
                ElkDirection::Right,
                ExternalPortData {
                    label_spacings: spacings,
                    ..ExternalPortData::default()
                },
            );
            for (width, height) in [(20.0, 10.0), (30.0, 6.0)] {
                dummy.ports[PortId::new(0)]
                    .labels
                    .push(LLabel::new("l", width, height));
            }
            dummy
        });
        graph.set_node_layer(dummy, 0);
        (graph, dummy)
    }

    fn spacings(horizontal: f64, vertical: f64, between: f64) -> DummyLabelSpacings {
        DummyLabelSpacings {
            label_port_horizontal: horizontal,
            label_port_vertical: vertical,
            label_label: between,
        }
    }

    /// Where the two labels lie after the placement.
    fn positions(graph: &LGraph, dummy: NodeId) -> Vec<(f64, f64)> {
        graph.layerless_nodes[dummy].ports[PortId::new(0)]
            .labels
            .iter()
            .map(|label| (label.position.x, label.position.y))
            .collect()
    }

    /// Gives the dummy's port an edge, so a label is no longer placed next to the port.
    fn connect(graph: &mut LGraph, dummy: NodeId) {
        let other = graph
            .push_node_with(|node| LNode::new(format!("other{}", node.index()), 10.0, 10.0, None));
        let port = graph.layerless_nodes[other]
            .ports
            .push(crate::graph::LPort::new("o", other, PortType::Output));
        graph
            .add_edge(LayeredEdge::new(
                "e",
                PortRef { node: other, port },
                PortRef {
                    node: dummy,
                    port: PortId::new(0),
                },
            ))
            .unwrap();
    }

    fn place(graph: &mut LGraph, dummy: NodeId, placement: PortLabelPlacement, group: bool) {
        let next_to_port = placement.contains(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);
        place_external_port_dummy_labels(graph, dummy, placement, next_to_port, group);
    }

    // Mutation: in the INSIDE north arm, leave out `- port_position.x`, or set `box_y` to the
    // negative spacing.
    #[test]
    fn inside_labels_of_a_north_dummy_sit_below_it_and_centre_on_the_port() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::North, spacings(1.0, 1.0, 0.0));
        place(&mut graph, dummy, PortLabelPlacement::INSIDE, true);
        // The box is 30 wide: (6 - 30) / 2 - 3 = -15. The labels stack from y = 1.
        assert_eq!(positions(&graph, dummy), vec![(-15.0, 1.0), (-15.0, 11.0)]);
    }

    // Mutation: in the INSIDE south arm, use `+ label_port_vertical` as the north arm does.
    #[test]
    fn inside_labels_of_a_south_dummy_sit_above_it() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::South, spacings(1.0, 1.0, 0.0));
        place(&mut graph, dummy, PortLabelPlacement::INSIDE, true);
        // The box is 16 high: y = -1 - 16.
        assert_eq!(
            positions(&graph, dummy),
            vec![(-15.0, -17.0), (-15.0, -7.0)]
        );
    }

    // Mutation: in the INSIDE east arm, leave out `- box_width`, or give the west arm the x of
    // the east arm.
    #[test]
    fn inside_labels_of_an_east_dummy_end_before_it_and_those_of_a_west_dummy_start_after_it() {
        let (mut east, east_dummy) = dummy_with_labels(PortSide::East, spacings(3.0, 1.0, 0.0));
        let (mut west, west_dummy) = dummy_with_labels(PortSide::West, spacings(3.0, 1.0, 0.0));
        connect(&mut east, east_dummy);
        connect(&mut west, west_dummy);
        place(&mut east, east_dummy, PortLabelPlacement::INSIDE, true);
        place(&mut west, west_dummy, PortLabelPlacement::INSIDE, true);
        // A connected port: below it. y = 4 (dummy height) + 1 - 2 (port y) = 3.
        assert_eq!(
            positions(&east, east_dummy),
            vec![(-33.0, 3.0), (-33.0, 13.0)]
        );
        assert_eq!(positions(&west, west_dummy), vec![(3.0, 3.0), (3.0, 13.0)]);
    }

    // Mutation: in `label_next_to_port`, ask for the incoming edges only, or ignore the edges.
    #[test]
    fn inside_labels_go_next_to_a_port_without_edges_and_below_one_with_edges() {
        let next = PortLabelPlacement::INSIDE | PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE;
        let (mut free, free_dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        place(&mut free, free_dummy, next, true);
        // The group is 16 high: y = (4 - 16) / 2 - 2 = -8.
        assert_eq!(positions(&free, free_dummy)[0].1, -8.0);

        let (mut incoming, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        connect(&mut incoming, dummy);
        place(&mut incoming, dummy, next, true);
        assert_eq!(positions(&incoming, dummy)[0].1, 3.0);
    }

    // Mutation: in `label_next_to_port`, read `outgoing_edges` alone.
    #[test]
    fn an_edge_that_leaves_the_dummy_port_counts_as_well() {
        let next = PortLabelPlacement::INSIDE | PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE;
        let (mut graph, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        let other =
            graph.push_node_with(|node| LNode::new(format!("o{}", node.index()), 10.0, 10.0, None));
        let port = graph.layerless_nodes[other]
            .ports
            .push(crate::graph::LPort::new("o", other, PortType::Input));
        graph
            .add_edge(LayeredEdge::new(
                "out",
                PortRef {
                    node: dummy,
                    port: PortId::new(0),
                },
                PortRef { node: other, port },
            ))
            .unwrap();
        place(&mut graph, dummy, next, true);
        assert_eq!(positions(&graph, dummy)[0].1, 3.0);
    }

    // Mutation: always use the height of the whole box, also without `treatAsGroup`.
    #[test]
    fn without_treat_as_group_the_first_label_decides_the_centre() {
        let next = PortLabelPlacement::INSIDE | PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE;
        let (mut graph, dummy) = dummy_with_labels(PortSide::West, spacings(1.0, 1.0, 0.0));
        place(&mut graph, dummy, next, false);
        // The first label is 10 high: y = (4 - 10) / 2 - 2 = -5.
        assert_eq!(positions(&graph, dummy)[0], (1.0, -5.0));
    }

    // Mutation: leave `label_label` out of the box height or out of the stacking step.
    #[test]
    fn the_spacing_between_labels_enters_the_box_and_the_stack() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::South, spacings(1.0, 1.0, 5.0));
        place(&mut graph, dummy, PortLabelPlacement::INSIDE, true);
        // The box is 10 + 6 + 5 = 21 high: y = -1 - 21. The second label follows after 10 + 5.
        assert_eq!(
            positions(&graph, dummy),
            vec![(-15.0, -22.0), (-15.0, -7.0)]
        );
    }

    // Mutation: in the OUTSIDE north and south arm, leave out `label_port_horizontal`.
    #[test]
    fn outside_labels_of_a_north_dummy_start_beside_the_port() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::North, spacings(2.0, 1.0, 0.0));
        place(&mut graph, dummy, PortLabelPlacement::OUTSIDE, true);
        // The port sits at x = 3 on its dummy. The labels stack from y = 0.
        assert_eq!(positions(&graph, dummy), vec![(5.0, 0.0), (5.0, 10.0)]);
    }

    // Mutation: in the OUTSIDE east and west arm, ask `label_next_to_port` for inside labels.
    #[test]
    fn outside_labels_go_next_to_a_port_that_no_outer_node_reaches() {
        let next = PortLabelPlacement::OUTSIDE | PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE;
        let (mut quiet, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        quiet.layerless_nodes[dummy].ports[PortId::new(0)].connected_to_external_nodes = false;
        place(&mut quiet, dummy, next, true);
        assert_eq!(positions(&quiet, dummy)[0], (0.0, -8.0));

        let (mut reached, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        reached.layerless_nodes[dummy].ports[PortId::new(0)].connected_to_external_nodes = true;
        place(&mut reached, dummy, next, true);
        // Below the port: y = 2 + 1.
        assert_eq!(positions(&reached, dummy)[0], (0.0, 3.0));
    }

    // Mutation: keep the position a label has when the placement is fixed. ELK stacks the labels
    // from the origin of the box in every case: neither arm moves the box.
    #[test]
    fn fixed_labels_are_stacked_from_the_origin_of_the_box() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        graph.layerless_nodes[dummy].ports[PortId::new(0)].labels[0].position =
            LPoint { x: 7.0, y: 8.0 };
        place(&mut graph, dummy, PortLabelPlacement::FIXED, true);
        assert_eq!(positions(&graph, dummy), vec![(0.0, 0.0), (0.0, 10.0)]);
    }

    // Mutation: place the labels of a dummy that has none, which panics on the first label.
    #[test]
    fn a_dummy_without_labels_is_left_alone() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::East, spacings(1.0, 1.0, 0.0));
        graph.layerless_nodes[dummy].ports[PortId::new(0)]
            .labels
            .clear();
        place(&mut graph, dummy, PortLabelPlacement::INSIDE, true);
        assert!(
            graph.layerless_nodes[dummy].ports[PortId::new(0)]
                .labels
                .is_empty()
        );
    }

    // Mutation: run the placement without the `EXTERNAL_PORTS` check.
    #[test]
    fn the_processor_places_dummy_labels_only_in_a_graph_with_external_ports() {
        let (mut plain, dummy) = dummy_with_labels(PortSide::North, spacings(1.0, 1.0, 0.0));
        plain.options.port_label_placement = PortLabelPlacement::INSIDE;
        calculate_label_and_node_sizes(&mut plain, OtherGraphs::none());
        assert_eq!(positions(&plain, dummy), vec![(0.0, 0.0), (0.0, 0.0)]);

        let (mut external, dummy) = dummy_with_labels(PortSide::North, spacings(1.0, 1.0, 0.0));
        external.options.port_label_placement = PortLabelPlacement::INSIDE;
        external.graph_properties.external_ports = true;
        calculate_label_and_node_sizes(&mut external, OtherGraphs::none());
        assert_eq!(
            positions(&external, dummy),
            vec![(-15.0, 1.0), (-15.0, 11.0)]
        );
    }

    // Mutation: pass `false` for `treat_as_group` whatever the option says.
    #[test]
    fn the_processor_reads_the_graph_options_for_the_placement() {
        let (mut graph, dummy) = dummy_with_labels(PortSide::West, spacings(1.0, 1.0, 0.0));
        graph.options.port_label_placement =
            PortLabelPlacement::INSIDE | PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE;
        graph.options.port_labels_treat_as_group = true;
        graph.graph_properties.external_ports = true;
        calculate_label_and_node_sizes(&mut graph, OtherGraphs::none());
        assert_eq!(positions(&graph, dummy)[0].1, -8.0);
    }
}
